use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::GzDecoder;
use lam_history_store::{Access, HistoryWrite, Namespace, PageQuery, Section, Store};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::tempdir;

const BINARY: &str = env!("CARGO_BIN_EXE_local-activity-monitor");
const TIMEOUT: Duration = Duration::from_secs(5);

struct Service {
    child: Child,
    port: u16,
}

impl Service {
    fn start(home: &Path, codex: bool, assets: Option<&Path>) -> Self {
        let mut command = Command::new(BINARY);
        command
            .args(["--port", "0", "--watch-stdin", "--codex-home"])
            .arg(home);
        if codex {
            command.arg("--codex");
        }
        if let Some(assets) = assets {
            command.arg("--assets").arg(assets);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = send.send(result);
        });
        let mut service = Self { child, port: 0 };
        let line = receive.recv_timeout(TIMEOUT).unwrap().unwrap();
        let ready: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(ready["status"], "listening");
        assert_eq!(ready["codex_metadata"], codex);
        service.port = ready["url"]
            .as_str()
            .unwrap()
            .strip_prefix("http://127.0.0.1:")
            .unwrap()
            .trim_end_matches('/')
            .parse()
            .unwrap();
        service
    }

    fn get(&self, path: &str) -> Reply {
        self.request(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n",
            self.port
        ))
    }

    fn request(&self, request: &str) -> Reply {
        self.request_body(request, "")
    }

    fn request_body(&self, request: &str, body: &str) -> Reply {
        let mut stream =
            TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, self.port).into(), TIMEOUT).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TIMEOUT)).unwrap();
        write!(stream, "{request}Connection: close\r\n\r\n{body}").unwrap();
        let mut data = Vec::new();
        stream.read_to_end(&mut data).unwrap();
        let split = data
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap();
        let headers = String::from_utf8(data[..split].to_vec()).unwrap();
        let status = headers
            .split_ascii_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        Reply {
            status,
            headers,
            body: data.split_off(split + 4),
        }
    }

    fn stop(&mut self) {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("service did not stop when stdin closed");
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct Reply {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

#[test]
fn monitor_journal_persists_lifecycle_and_loads_it_on_restart() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    let first = service.get("/api/logs?window=all").json();
    assert!(first["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["source"] == "monitor" && entry["kind"] == "started"));
    service.stop();
    let path = home.path().join("monitoring/local-activity-monitor.jsonl");
    let records = fs::read_to_string(path).unwrap();
    assert!(records
        .lines()
        .any(|line| serde_json::from_str::<Value>(line).unwrap()["kind"] == "stopped"));
    let restarted = Service::start(home.path(), false, None);
    let logs = restarted.get("/api/logs?window=all").json();
    assert!(logs["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "stopped"));
}

#[test]
fn frontend_diagnostics_validate_deduplicate_and_publish_metadata() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/diagnostics HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()), &body)
    };
    let report = json!({"phase":"render","error_type":"TypeError","frontend_revision":null,"frames":[{"file":"app.js","function":"render","line":2,"column":3}]});
    let first = post(report.clone());
    assert_eq!(first.status, 202);
    assert_eq!(
        serde_json::from_slice::<Value>(&first.body).unwrap()["recorded"],
        true
    );
    let duplicate = post(report.clone());
    assert_eq!(
        serde_json::from_slice::<Value>(&duplicate.body).unwrap()["recorded"],
        false
    );
    let mut invalid = report;
    invalid["message"] = json!("private");
    assert_eq!(post(invalid).status, 400);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let logs = service.get("/api/logs?window=all").json();
        if logs["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "frontend_error")
        {
            assert!(!logs.to_string().contains("private"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn http_error_journal_deduplicates_consecutive_status_without_request_data() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    assert_eq!(service.get("/missing-private-path").status, 404);
    assert_eq!(service.get("/missing-private-path").status, 404);
    assert_eq!(service.get("/api/snapshot?window=invalid").status, 400);
    assert_eq!(service.get("/missing-private-path").status, 404);
    let journal =
        fs::read_to_string(home.path().join("monitoring/local-activity-monitor.jsonl")).unwrap();
    let recorded: Vec<Value> = journal
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|entry: &Value| entry["kind"] == "http_response_error")
        .collect();
    assert_eq!(
        recorded
            .iter()
            .map(|entry| entry["http_status"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![404, 400, 404]
    );
    assert!(!journal.contains("missing-private-path"));
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let logs = service.get("/api/logs?window=all").json();
        let errors: Vec<_> = logs["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["kind"] == "http_response_error")
            .collect();
        if !errors.is_empty() {
            assert!(!logs.to_string().contains("missing-private-path"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "unexpected HTTP error entries: {errors:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn settings_journal_records_only_successful_applications() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()), &body)
    };
    assert_eq!(post(json!({"observations":{"metadata":false}})).status, 200);
    assert_eq!(post(json!({"activity_retention_days":3})).status, 200);
    assert_eq!(post(json!({"activity_retention_days":-1})).status, 409);
    let records =
        fs::read_to_string(home.path().join("monitoring/local-activity-monitor.jsonl")).unwrap();
    let settings: Vec<Value> = records
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .filter(|entry: &Value| entry["kind"] == "settings_applied")
        .collect();
    assert_eq!(settings.len(), 2);
    assert!(settings
        .iter()
        .all(|entry| entry.get("activity_retention_days").is_none()
            && entry.get("observations").is_none()));
}

#[test]
fn log_setting_stops_disk_writes_and_hides_entries_until_reenabled() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let post = |route: &str, value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST {route} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    assert_eq!(
        post("/api/settings", json!({"observations":{"logs":false}})).status,
        200
    );
    let report =
        json!({"phase":"script","error_type":"TypeError","frontend_revision":null,"frames":[]});
    assert_eq!(post("/api/diagnostics", report).status, 202);
    let disabled = service.get("/api/logs?window=all").json();
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["entries"], json!([]));
    let path = home.path().join("monitoring/local-activity-monitor.jsonl");
    assert!(!fs::read_to_string(&path)
        .unwrap()
        .contains("frontend_error"));
    assert_eq!(
        post("/api/settings", json!({"observations":{"logs":true}})).status,
        200
    );
    let enabled = service.get("/api/logs?window=all").json();
    assert_eq!(enabled["enabled"], true);
    assert!(enabled["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "frontend_error"));
    assert!(!fs::read_to_string(path).unwrap().contains("frontend_error"));
}

#[test]
fn reenabled_logs_rescan_core_and_desktop_sources() {
    let home = tempdir().unwrap();
    fs::create_dir(home.path().join("desktop-logs")).unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let timestamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(
        home.path().join("desktop-logs/codex-desktop-fixture.log"),
        format!("{timestamp} INFO [codex] fixture\n"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(home.path().join("logs_active.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT, message TEXT)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO logs VALUES(1,?,'INFO','codex_core','fixture')",
        [now.timestamp_millis() as f64 / 1000.0],
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let has_both = |logs: &Value| {
        ["codex_core", "codex_desktop"].iter().all(|source| {
            logs["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["source"] == *source)
        })
    };
    let wait_logs = || {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let logs = service.get("/api/logs?window=all").json();
            if has_both(&logs) {
                break;
            }
            assert!(Instant::now() < deadline, "missing sources: {logs}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let set = |enabled| {
        let body = json!({"observations":{"logs":enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()), &body)
    };
    wait_logs();
    assert_eq!(set(false).status, 200);
    assert_eq!(
        service.get("/api/logs?window=all").json()["entries"],
        json!([])
    );
    assert_eq!(set(true).status, 200);
    wait_logs();
}

#[test]
fn debug_setting_records_collection_only_when_enabled() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let path = home.path().join("monitoring/performance-debug.jsonl");
    assert!(!path.exists());
    let set = |enabled| {
        let body = json!({"debug_mode":enabled}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()), &body)
    };
    assert_eq!(set(true).status, 200);
    let snapshot = service.get("/api/snapshot").json();
    assert_eq!(snapshot["settings"]["debug_mode"], true);
    assert!(snapshot["monitor"]["debug"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["kind"] == "collection"));
    assert_eq!(set(false).status, 200);
    assert_eq!(
        service.get("/api/snapshot").json()["settings"]["debug_mode"],
        false
    );
    let records = fs::read_to_string(path).unwrap();
    assert!(records
        .lines()
        .any(|line| serde_json::from_str::<Value>(line).unwrap()["kind"] == "disabled"));
}

#[test]
fn debug_stream_reports_connection_and_snapshot() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let body = r#"{"debug_mode":true}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    let mut stream = EventStream::connect(&service, "").unwrap();
    stream.snapshot();
    let frame = stream.frame();
    let payload = frame.strip_prefix("event: debug\ndata: ").unwrap().trim();
    let debug: Value = serde_json::from_str(payload).unwrap();
    assert_eq!(debug["active_streams"], 1);
    let event = debug["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "stream_frame")
        .unwrap();
    assert!(event["snapshot_bytes"].as_u64().unwrap() > 0);
    assert!(event.get("transfer_bytes").is_none());
    drop(stream);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let body = "{}";
        service.request_body(&format!("POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body);
        if service.get("/api/snapshot").json()["monitor"]["debug"]["active_streams"] == 0 {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn debug_collection_reports_only_measured_phase_metrics() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), true, None);
    let body = r#"{"debug_mode":true}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    let snapshot = snapshot_when(&service, |value| {
        value["monitor"]["debug"]["records"]
            .as_array()
            .is_some_and(|records| {
                records.iter().any(|record| {
                    record["kind"] == "collection"
                        && record["phase_metrics"]["sessions"].is_object()
                })
            })
    });
    let record = snapshot["monitor"]["debug"]["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| {
            record["kind"] == "collection" && record["phase_metrics"]["sessions"].is_object()
        })
        .unwrap();
    for phase in ["sessions", "diagnostics", "history"] {
        assert!(record["phase_metrics"][phase]["elapsed_ms"]
            .as_f64()
            .is_some_and(|value| value >= 0.0));
        assert!(record["phase_metrics"][phase]["cpu_ms"]
            .as_f64()
            .is_some_and(|value| value >= 0.0));
    }
    assert!(record["phase_metrics"].get("worktrees").is_none());
}

#[test]
fn dots_query_limits_metadata_filters_windows_and_honors_settings() {
    let home = tempdir().unwrap();
    let now =
        chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).timestamp_millis();
    let id = "00000000-0000-4000-8000-000000000001";
    let state = json!({"electron-persisted-atom-state":{
        "orbit-outputs-v1":[{"threadId":id,"hostId":"remote-control:fixture","artifact":{"type":"text","producedAtMs":now,"content":"PRIVATE_CONTENT"},"accountId":"PRIVATE_ACCOUNT"}],
        "orbit-activity-snapshots-v1":[{"data":[{"threadId":id,"hostId":"durable","timestampMs":now},{"threadId":id,"hostId":"durable","timestampMs":now-172800000},{"threadId":id,"hostId":"https://invalid","timestampMs":now}]}]
    }});
    fs::write(
        home.path().join(".codex-global-state.json"),
        state.to_string(),
    )
    .unwrap();
    let service = Service::start(home.path(), false, None);
    let all = service.get("/api/codex/dots?window=all").json();
    assert_eq!(all["health"], "ok");
    assert_eq!(all["events"].as_array().unwrap().len(), 1);
    assert_eq!(all["activity"].as_array().unwrap().len(), 3);
    assert!(!all.to_string().contains("PRIVATE"));
    let recent = service.get("/api/codex/dots?window=24h").json();
    assert_eq!(recent["activity"].as_array().unwrap().len(), 2);
    assert_eq!(service.get("/api/codex/dots?window=bad").status, 400);
    let body = r#"{"observations":{"metadata":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    assert_eq!(
        service.get("/api/codex/dots").json(),
        json!({"health":"disabled"})
    );
}

#[test]
fn dots_background_merges_catalog_and_updates_with_source() {
    let home = tempdir().unwrap();
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let now =
        chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).timestamp_millis();
    let db = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    db.execute_batch("CREATE TABLE automations(id TEXT, status TEXT); CREATE TABLE local_thread_catalog(host_id TEXT,thread_id TEXT,display_title TEXT,source_created_at INTEGER,source_updated_at INTEGER,source_kind TEXT,thread_source TEXT,project_id TEXT);").unwrap();
    db.execute("INSERT INTO local_thread_catalog VALUES('remote-control:fixture',?,'title',0,?,'cli','user',NULL)",rusqlite::params![id,now as f64 / 1000.0]).unwrap();
    let path = home.path().join(".codex-global-state.json");
    let state = |kind| {
        json!({"electron-persisted-atom-state":{"orbit-outputs-v1":[{"threadId":id,"artifact":{"type":kind,"producedAtMs":now,"content":"PRIVATE"}}]}}).to_string()
    };
    fs::write(&path, state("text")).unwrap();
    let service = Service::start(home.path(), true, None);
    let initial = snapshot_when(&service, |value| {
        value["codex"]["dots"]["events"][0]["artifact_type"] == "text"
    });
    assert_eq!(initial["codex"]["dots"]["computers"][0]["thread_count"], 1);
    assert!(initial["codex"]["dots"]["activity"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["activity_type"] == "dot"));
    assert!(!initial["codex"]["dots"].to_string().contains("PRIVATE"));
    fs::write(path, state("image")).unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["dots"]["events"][0]["artifact_type"] == "image"
    });
    let body = r#"{"observations":{"metadata":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    assert_eq!(
        service.get("/api/snapshot?window=all").json()["codex"]["dots"]["activity"],
        json!([])
    );
}

#[test]
fn catalog_metadata_enriches_observed_threads_and_adds_metadata_only_rows() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    append_records(
        &session,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"fixture","call_id":"call-1"}),
        )],
    );
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let db = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    db.execute_batch("CREATE TABLE automations(id TEXT,status TEXT,target_thread_id TEXT); CREATE TABLE local_thread_catalog(host_id TEXT,thread_id TEXT,display_title TEXT,source_created_at INTEGER,source_updated_at INTEGER,source_kind TEXT,thread_source TEXT,project_id TEXT,model TEXT,reasoning_effort TEXT,model_provider TEXT,chatgpt_async_status TEXT,archived INTEGER,private_data TEXT); INSERT INTO local_thread_catalog VALUES('local','00000000-0000-4000-8000-000000000002','Local title',1700000000,1700000010,'cli','user','project-a','model-a','high','openai','idle',0,'PRIVATE'); INSERT INTO local_thread_catalog VALUES('durable','00000000-0000-4000-8000-000000000003','Cloud title',1700000000,1700000020,'chatgpt','schedule',NULL,'model-b','low','openai','running',1,'PRIVATE'); INSERT INTO automations VALUES('schedule-a','ACTIVE','00000000-0000-4000-8000-000000000002');").unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["threads"]
            .as_array()
            .is_some_and(|rows| rows.len() == 2 && rows.iter().any(|row| row["tool_calls"] == 1))
    });
    let rows = snapshot["codex"]["threads"].as_array().unwrap();
    let local = rows
        .iter()
        .find(|row| row["thread_id"] == "00000000-0000-4000-8000-000000000002")
        .unwrap();
    assert_eq!(local["thread_name"], "Local title");
    assert_eq!(local["metadata_only"], false);
    assert_eq!(local["has_schedule"], true);
    assert_eq!(local["tool_calls"], 1);
    let cloud = rows
        .iter()
        .find(|row| row["thread_id"] == "00000000-0000-4000-8000-000000000003")
        .unwrap();
    assert_eq!(cloud["metadata_only"], true);
    assert_eq!(cloud["environment"], "cloud");
    assert_eq!(cloud["model"], "model-b");
    assert_eq!(cloud["archived"], true);
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let body = r#"{"observations":{"metadata":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["codex"]["threads"].as_array().unwrap().len(), 1);
}

#[test]
fn state_metadata_projects_execution_and_parent_without_raw_policies() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    append_records(
        &session,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"fixture","call_id":"call-1"}),
        )],
    );
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let catalog = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    catalog.execute_batch("CREATE TABLE automations(id TEXT,status TEXT); CREATE TABLE local_thread_catalog(host_id TEXT,thread_id TEXT,display_title TEXT,source_created_at INTEGER,source_updated_at INTEGER,source_kind TEXT,thread_source TEXT,project_id TEXT); INSERT INTO local_thread_catalog VALUES('local','00000000-0000-4000-8000-000000000003','Child',0,1700000000,'cli','user',NULL);").unwrap();
    let db = rusqlite::Connection::open(home.path().join("state_1.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,model TEXT,reasoning_effort TEXT,originator TEXT,model_provider TEXT,approval_mode TEXT,sandbox_policy TEXT,git_branch TEXT,git_sha TEXT,agent_nickname TEXT,agent_path TEXT,source TEXT,archived INTEGER,created_at_ms INTEGER); CREATE TABLE thread_spawn_edges(parent_thread_id TEXT,child_thread_id TEXT); INSERT INTO thread_spawn_edges VALUES('00000000-0000-4000-8000-000000000002','00000000-0000-4000-8000-000000000003');").unwrap();
    let source=json!({"subagent":{"thread_spawn":{"agent_role":"reviewer","agent_nickname":"from-source","secret":"PRIVATE_SECRET"}}}).to_string();
    let sandbox = json!({"type":"workspace-write","writable_roots":["PRIVATE_ROOT"]}).to_string();
    db.execute("INSERT INTO threads VALUES('00000000-0000-4000-8000-000000000003','Child','state-model','high','codex_work_desktop','openai','on-request',?1,'main','abcdef123456','from-state','/root/child',?2,1,1700000000000)",rusqlite::params![sandbox,source]).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["thread_id"] == "00000000-0000-4000-8000-000000000003"
                    && row["execution"]["parent_thread_id"].is_string()
            })
        })
    });
    let child = snapshot["codex"]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["thread_id"] == "00000000-0000-4000-8000-000000000003")
        .unwrap();
    assert_eq!(child["trigger"], "subagent");
    assert_eq!(
        child["execution"]["parent_thread_id"],
        "00000000-0000-4000-8000-000000000002"
    );
    assert_eq!(child["execution"]["agent_nickname"], "from-state");
    assert_eq!(child["execution"]["agent_role"], "reviewer");
    assert_eq!(child["execution"]["sandbox_mode"], "workspace-write");
    assert_eq!(child["model"], "state-model");
    assert_eq!(child["archived"], true);
    assert_eq!(child["activity_type"], "work");
    assert!(!snapshot.to_string().contains("PRIVATE_ROOT"));
    assert!(!snapshot.to_string().contains("PRIVATE_SECRET"));
}

#[test]
fn session_execution_metadata_is_selected_and_survives_checkpoint_restore() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let rows = [
        event(
            &stamp,
            "session_meta",
            json!({"id":id,"model_provider":"openai","cli_version":"0.160.0","parent_thread_id":id,"git":{"branch":"feature/test","commit_hash":"abcdef123456","repository_url":"PRIVATE_URL"},"base_instructions":"PRIVATE_PROMPT","source":{"subagent":{"thread_spawn":{"agent_role":"reviewer","private":"PRIVATE_SOURCE"}}}}),
        ),
        event(
            &stamp,
            "turn_context",
            json!({"model":"session-model","effort":"high","sandbox_policy":{"type":"workspace-write","writable_roots":["PRIVATE_ROOT"]},"collaboration_mode":{"mode":"default","settings":{"developer_instructions":"PRIVATE_TEXT"}}}),
        ),
        event(
            &stamp,
            "event_msg",
            json!({"type":"token_count","info":{"model_context_window":272000}}),
        ),
    ];
    fs::write(path, rows.concat()).unwrap();
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["execution"]["context_window"] == 272000)
        }) && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let row = &snapshot["codex"]["threads"][0];
    assert_eq!(row["execution"]["sandbox_mode"], "workspace-write");
    assert_eq!(row["execution"]["collaboration_mode"], "default");
    assert_eq!(row["execution"]["model_provider"], "openai");
    assert_eq!(row["execution"]["git_branch"], "feature/test");
    assert_eq!(row["execution"]["agent_role"], "reviewer");
    assert!(!snapshot.to_string().contains("PRIVATE"));
    service.stop();
    let mut restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(
        restored["codex"]["threads"][0]["execution"]["context_window"],
        272000
    );
    assert!(!restored.to_string().contains("PRIVATE"));
    restarted.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    db.execute("UPDATE session_cursors SET payload=json_remove(json_set(payload,'$.version',4),'$.projection.execution')",[]).unwrap();
    drop(db);
    let replayed = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&replayed, |value| {
        value["codex"]["read_state"]["checkpoint"]["invalidated"] == 1
            && value["codex"]["threads"].as_array().is_some_and(|rows| {
                rows.iter()
                    .any(|row| row["execution"]["context_window"] == 272000)
            })
    });
    assert!(snapshot["codex"]["bytes_read"].as_u64().unwrap() > 0);
}

#[test]
fn project_snapshot_merges_authoritative_names_and_hides_project_paths() {
    let home = tempdir().unwrap();
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let catalog = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    catalog.execute_batch("CREATE TABLE automations(id TEXT,status TEXT); CREATE TABLE local_thread_catalog(host_id TEXT,thread_id TEXT,display_title TEXT,source_created_at INTEGER,source_updated_at INTEGER,source_kind TEXT,thread_source TEXT,project_id TEXT); INSERT INTO local_thread_catalog VALUES('local','00000000-0000-4000-8000-000000000001','Fixture',0,1700000000,'cli','user','legacy');").unwrap();
    let state = rusqlite::Connection::open(home.path().join("state_5.sqlite")).unwrap();
    state.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT); INSERT INTO threads VALUES('00000000-0000-4000-8000-000000000001','Fixture','legacy'); CREATE TABLE projects(id TEXT,name TEXT,updated_at_ms INTEGER); INSERT INTO projects VALUES('canonical','Database name',1700000000000); CREATE TABLE project_roots(project_id TEXT,path TEXT); INSERT INTO project_roots VALUES('canonical','/fixture/owned-root');").unwrap();
    let global = json!({"local-projects":{"legacy":{"name":"Old name","rootPaths":["PRIVATE_PATH"]}},"app-server-project-id-by-legacy-project-id-by-host":{"local":{"legacy":"canonical"}},"thread-project-assignments":{id:{"projectId":"legacy","projectKind":"workspace","projectOrigin":"local"}},"electron-persisted-atom-state":{}});
    fs::write(
        home.path().join(".codex-global-state.json"),
        global.to_string(),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["threads"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["project_id"] == "canonical"))
    });
    let thread = snapshot["codex"]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["thread_id"] == id)
        .unwrap();
    assert_eq!(thread["project_name"], "Database name");
    assert_eq!(thread["project_kind"], "workspace");
    let project = snapshot["codex"]["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "canonical")
        .unwrap();
    assert_eq!(project["name"], "Database name");
    assert_eq!(project["thread_count"], 1);
    assert_eq!(project["origin"], "local");
    assert!(!snapshot.to_string().contains("PRIVATE_PATH"));
    assert!(!snapshot.to_string().contains("/fixture/owned-root"));
    let detail = service
        .get("/api/codex/project?project_id=canonical")
        .json();
    assert_eq!(detail["folders"], json!(["/fixture/owned-root"]));
    assert_eq!(detail["source"], "projects");
    assert_eq!(detail["threads"][0]["thread_id"], id);
    assert!(!detail.to_string().contains("PRIVATE_PATH"));
    assert_eq!(
        service.get("/api/codex/project?project_id=unknown").status,
        409
    );
    assert_eq!(
        service
            .get("/api/codex/project?project_id=..%2Foutside")
            .status,
        400
    );
    assert_eq!(
        service
            .get("/api/codex/project?project_id=canonical&path=anything")
            .status,
        400
    );
    state.execute("DELETE FROM project_roots", []).unwrap();
    let fallback = service
        .get("/api/codex/project?project_id=canonical")
        .json();
    assert_eq!(fallback["folders"], json!(["PRIVATE_PATH"]));
    let body = r#"{"observations":{"metadata":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    assert_eq!(
        service
            .get("/api/codex/project?project_id=canonical")
            .status,
        409
    );
}

#[test]
fn instructions_are_bounded_redacted_and_limited_to_observed_roots() {
    let temporary = tempdir().unwrap();
    let home = temporary.path().canonicalize().unwrap();
    let root = home.join("workspace");
    fs::create_dir(&root).unwrap();
    fs::write(home.join("AGENTS.md"), "global guidance").unwrap();
    fs::write(root.join("AGENTS.md"), "base content").unwrap();
    fs::write(
        root.join("AGENTS.override.md"),
        format!(
            "api_key=PRIVATE_PASSWORD\nBearer PRIVATE_TOKEN_VALUE\nsk-1234567890abcdef\n{}",
            "x".repeat(70000)
        ),
    )
    .unwrap();
    let global = json!({"local-projects":{"p1":{"name":"Fixture","rootPaths":[root]}},"electron-persisted-atom-state":{}});
    fs::write(home.join(".codex-global-state.json"), global.to_string()).unwrap();
    let service = Service::start(&home, true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["projects"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["id"] == "p1"))
    });
    assert!(!snapshot.to_string().contains("PRIVATE_PASSWORD"));
    let details = service
        .get("/api/codex/instructions?scope=project&project_id=p1")
        .json();
    let doc = &details["documents"][0];
    assert_eq!(doc["filename"], "AGENTS.override.md");
    assert_eq!(doc["read_bytes"], 65536);
    assert_eq!(doc["truncated"], true);
    assert!(!details.to_string().contains("PRIVATE_PASSWORD"));
    assert!(!details.to_string().contains("PRIVATE_TOKEN_VALUE"));
    assert!(!details.to_string().contains("sk-1234567890abcdef"));
    assert_eq!(
        service.get("/api/codex/instructions?scope=global").json()["documents"][0]["text"],
        "global guidance"
    );
    assert_eq!(
        service
            .get("/api/codex/instructions?scope=project&project_id=unknown")
            .status,
        409
    );
    assert_eq!(
        service
            .get("/api/codex/instructions?scope=project&project_id=..%2Foutside")
            .status,
        400
    );
    assert_eq!(
        service
            .get("/api/codex/instructions?scope=global&file=auth.json")
            .status,
        400
    );
}

#[cfg(unix)]
#[test]
fn dots_query_rejects_redirected_global_state() {
    let home = tempdir().unwrap();
    let source = home.path().join("other.json");
    fs::write(&source, "{}").unwrap();
    std::os::unix::fs::symlink(&source, home.path().join(".codex-global-state.json")).unwrap();
    let service = Service::start(home.path(), false, None);
    assert_eq!(service.get("/api/codex/dots").json()["health"], "error");
    assert_eq!(fs::read_to_string(source).unwrap(), "{}");
}

impl Reply {
    fn json(&self) -> Value {
        assert_eq!(self.status, 200, "{}", String::from_utf8_lossy(&self.body));
        serde_json::from_slice(&self.body).unwrap()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    }
}

#[test]
fn history_paginates_the_owned_database_and_preserves_the_wire_format() {
    let home = tempdir().unwrap();
    let stamp = "2026-01-01T00:00:00.000Z";
    {
        let mut store = Store::open(&home.path().join("monitoring"), Access::ReadWrite)
            .unwrap()
            .unwrap();
        let rows = [
            HistoryWrite {
                section: Section::Web,
                key: "a",
                ordinal: 0,
                event_time: Some(stamp),
                payload: r#"{"id":"a","token":0}"#,
            },
            HistoryWrite {
                section: Section::Web,
                key: "b",
                ordinal: 1,
                event_time: Some(stamp),
                payload: r#"{"id":"b","token":null}"#,
            },
            HistoryWrite {
                section: Section::Web,
                key: "c",
                ordinal: 2,
                event_time: None,
                payload: r#"{"id":"c","recorded":false}"#,
            },
        ];
        store
            .save_history(
                Namespace::Activity,
                r#"{"version":3,"retention_days":0}"#,
                0,
                None,
                &rows,
            )
            .unwrap();
    }
    let mut service = Service::start(home.path(), true, None);
    let first = service.get("/api/history?section=web&limit=1").json();
    assert_eq!(first["items"], json!([{"id":"b","token":null}]));
    assert_eq!(first["total"], 3);
    assert_eq!(first["limit"], 1);
    assert_eq!(first["offset"], 0);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = service
        .get(&format!("/api/history?section=web&limit=2&cursor={cursor}"))
        .json();
    assert_eq!(
        second["items"],
        json!([{"id":"a","token":0},{"id":"c","recorded":false}])
    );
    assert!(second["next_cursor"].is_null());
    let offset_date = service
        .get("/api/history?section=web&since=2026-01-01T08%3A00%3A00%2B08%3A00")
        .json();
    assert_eq!(offset_date["total"], 2);
    let threads = service
        .get("/api/history?namespace=threads&section=entries")
        .json();
    assert_eq!(
        threads,
        json!({"items":[],"total":0,"limit":100,"offset":0,"next_cursor":null})
    );
    service.stop();
}

#[test]
fn refresh_requires_local_empty_json_and_preserves_codex_opt_in() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let mut service = Service::start(home.path(), false, None);
    let post = |body: &str, headers: &str| {
        service.request_body(
            &format!(
                "POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}",
                service.port, body.len(),
            ),
            body,
        )
    };
    let snapshot = service.get("/api/snapshot").json();
    assert!(snapshot["updated_at"].is_string());
    assert_eq!(snapshot["codex"]["health"], "disabled");
    assert!(snapshot["activity"]["last_activity_at"].is_null());
    let mut stream = EventStream::connect(&service, "?window=24h").unwrap();
    let initial = stream.snapshot();
    assert_eq!(initial["window"], "24h");
    assert_eq!(initial["snapshot"], snapshot);
    assert_eq!(initial["snapshot"]["codex"]["threads"], json!([]));
    drop(stream);

    let response = post("{ }", "");
    assert_eq!(response.status, 200);
    assert_eq!(response.json(), service.get("/api/activity").json());
    assert_eq!(post("[]", "").status, 400);
    assert_eq!(post(&" ".repeat(513), "").status, 400);
    assert_eq!(
        post("{}", "Origin: http://external.invalid\r\n").status,
        403
    );
    assert!(!home.path().join("monitoring").exists());
    service.stop();
}

#[test]
fn invalid_requests_are_rejected_before_accessing_history() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    let local = format!("127.0.0.1:{}", service.port);
    let history = "/api/history?section=web";
    assert_eq!(
        service
            .request(&format!(
                "GET {history} HTTP/1.1\r\nHost: localhost:{}\r\nOrigin: http://localhost:{}\r\n",
                service.port, service.port
            ))
            .status,
        200
    );
    let refresh = "{}";
    assert_eq!(service.request_body(&format!("POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,refresh.len()),refresh).status,200);
    for headers in [
        "Host: external.invalid\r\n".into(),
        format!("Host: {local}\r\nOrigin: http://external.invalid\r\n"),
        format!("Host: {local}\r\nOrigin: null\r\n"),
        format!("Host: {local}\r\nSec-Fetch-Site: cross-site\r\n"),
    ] {
        assert_eq!(
            service
                .request(&format!("GET {history} HTTP/1.1\r\n{headers}"))
                .status,
            403
        );
    }
    for query in [
        "section=web&limit=201",
        "section=web&limit=1&limit=2",
        "section=web&since=2026-01-01",
        "section=web&cursor=%3F",
        "section=web&extra=1",
        "namespace=threads&section=entries",
    ] {
        assert_eq!(
            service.get(&format!("/api/history?{query}")).status,
            400,
            "{query}"
        );
    }
    assert_eq!(
        service
            .request(&format!(
                "POST {history} HTTP/1.1\r\nHost: {local}\r\nContent-Length: 0\r\n"
            ))
            .status,
        405
    );
    assert!(!home.path().join("monitoring").exists());
    service.stop();
}

#[test]
fn existing_matching_service_is_reused_without_starting_another() {
    let home = tempdir().unwrap();
    let other = tempdir().unwrap();
    let mut first = Service::start(home.path(), true, None);
    let matching = Command::new(BINARY)
        .args(["--codex-home"])
        .arg(home.path())
        .args(["--port", &first.port.to_string(), "--assets"])
        .arg(home.path().join("missing-assets"))
        .output()
        .unwrap();
    assert!(matching.status.success());
    let response: Value = serde_json::from_slice(&matching.stdout).unwrap();
    assert_eq!(response["status"], "already_running");
    assert_eq!(response["url"], format!("http://127.0.0.1:{}/", first.port));
    assert_eq!(first.get("/api/instance").status, 200);

    let different = Command::new(BINARY)
        .args(["--codex-home"])
        .arg(other.path())
        .args(["--port", &first.port.to_string()])
        .output()
        .unwrap();
    assert!(!different.status.success());
    assert!(!other.path().join("monitoring").exists());
    assert_eq!(first.get("/api/instance").status, 200);
    first.stop();
}

#[cfg(unix)]
#[test]
fn open_option_calls_configured_browser_for_new_and_existing_service() {
    assert!(Path::new("/usr/bin/printf").is_file());
    let home = tempdir().unwrap();
    let started = Command::new(BINARY)
        .args(["--codex-home"])
        .arg(home.path())
        .args(["--port", "0", "--watch-stdin", "--open"])
        .env("BROWSER", "/usr/bin/printf %s")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(started.status.success());
    let output = String::from_utf8(started.stdout).unwrap();
    let (receipt, opened) = output.split_once('\n').unwrap();
    let receipt: Value = serde_json::from_str(receipt).unwrap();
    assert_eq!(receipt["status"], "listening");
    assert_eq!(opened, receipt["url"].as_str().unwrap());

    let mut first = Service::start(home.path(), true, None);
    let mut command = Command::new(BINARY);
    command.args(["--codex-home"]).arg(home.path()).args([
        "--port",
        &first.port.to_string(),
        "--open",
    ]);
    let reused = command
        .env("BROWSER", "/usr/bin/printf %s")
        .output()
        .unwrap();
    assert!(reused.status.success());
    let output = String::from_utf8(reused.stdout).unwrap();
    let (receipt, opened) = output.split_once('\n').unwrap();
    let receipt: Value = serde_json::from_str(receipt).unwrap();
    assert_eq!(receipt["status"], "already_running");
    assert_eq!(opened, receipt["url"].as_str().unwrap());
    assert_eq!(first.get("/api/instance").status, 200);

    let without_open = Command::new(BINARY)
        .args(["--codex-home"])
        .arg(home.path())
        .args(["--port", &first.port.to_string()])
        .env("BROWSER", "/usr/bin/printf %s")
        .output()
        .unwrap();
    assert!(without_open.status.success());
    let receipt: Value = serde_json::from_slice(&without_open.stdout).unwrap();
    assert_eq!(receipt["status"], "already_running");
    first.stop();
}

fn fixture_home_id(home: &Path) -> String {
    let canonical = home.canonicalize().unwrap();
    let path = canonical.to_str().unwrap();
    #[cfg(windows)]
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}").to_lowercase()
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_lowercase()
    };
    format!("{:x}", Sha256::digest(path.as_bytes()))[..24].to_owned()
}

#[test]
fn oversized_instance_response_does_not_trigger_reuse() {
    let home = tempdir().unwrap();
    let home_id = fixture_home_id(home.path());
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release, wait) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        let body = json!({"application":"local-activity-monitor","home_id":home_id,"padding":"x".repeat(5000)}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        drop(stream);
        let _ = wait.recv_timeout(TIMEOUT);
    });
    let output = Command::new(BINARY)
        .args(["--codex-home"])
        .arg(home.path())
        .args(["--port", &port.to_string()])
        .output()
        .unwrap();
    release.send(()).unwrap();
    worker.join().unwrap();
    assert!(!output.status.success());
    assert!(!home.path().join("monitoring").exists());
}

#[test]
fn chunked_instance_response_can_be_reused() {
    let home = tempdir().unwrap();
    let home_id = fixture_home_id(home.path());
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        let body = json!({"application":"local-activity-monitor","home_id":home_id}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n", body.len(), body).unwrap();
    });
    let output = Command::new(BINARY)
        .args(["--codex-home"])
        .arg(home.path())
        .args(["--port", &port.to_string()])
        .output()
        .unwrap();
    worker.join().unwrap();
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "already_running");
    assert_eq!(result["url"], format!("http://127.0.0.1:{port}/"));
    assert!(!home.path().join("monitoring").exists());
}

#[test]
fn stdin_control_pauses_resumes_and_stops_the_owned_service() {
    for stop_with_restart in [false, true] {
        let home = tempdir().unwrap();
        let (path, stamp) = native_event_fixture(home.path());
        let mut child = Command::new(BINARY)
            .args(["--codex-home"])
            .arg(home.path())
            .args(["--port", "0", "--codex", "--watch-stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let first: Value = serde_json::from_str(&receive.recv_timeout(TIMEOUT).unwrap()).unwrap();
        let url = first["url"].as_str().unwrap().to_owned();
        let port = url
            .trim_end_matches('/')
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let mut service = Service { child, port };
        snapshot_when(&service, |value| value["codex"]["health"] == "ok");
        let mut stream = EventStream::connect(&service, "").unwrap();
        stream.snapshot();
        let mut slow = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        slow.set_read_timeout(Some(TIMEOUT)).unwrap();
        write!(slow, "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n").unwrap();
        assert_eq!(service.get("/api/instance").status, 200);
        let pid = service.child.id();
        service
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(format!("{}pause{}\n", " ".repeat(4096), "\u{2003}".repeat(1024)).as_bytes())
            .unwrap();
        let paused: Value = serde_json::from_str(&receive.recv_timeout(TIMEOUT).unwrap()).unwrap();
        assert_eq!(paused["status"], "paused");
        let mut final_chunk = String::new();
        stream.0.read_line(&mut final_chunk).unwrap();
        assert_eq!(final_chunk, "0\r\n");
        drop(stream);
        let mut closed = [0u8; 1];
        assert_eq!(slow.read(&mut closed).unwrap(), 0);
        drop(slow);
        assert!(TcpStream::connect_timeout(
            &(Ipv4Addr::LOCALHOST, port).into(),
            Duration::from_millis(250)
        )
        .is_err());
        append_records(
            &path,
            &[event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"fixture","call_id":"during-pause"}),
            )],
        );
        service
            .child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"resume\n")
            .unwrap();
        let resumed: Value = serde_json::from_str(&receive.recv_timeout(TIMEOUT).unwrap()).unwrap();
        assert_eq!(resumed["status"], "listening");
        assert_eq!(resumed["url"], url);
        assert_eq!(service.child.id(), pid);
        snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 1);
        if stop_with_restart {
            service
                .child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"restart\n")
                .unwrap();
            let deadline = Instant::now() + TIMEOUT;
            while service.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(service.child.try_wait().unwrap().is_some());
        } else {
            service
                .child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"pause\n")
                .unwrap();
            let paused: Value =
                serde_json::from_str(&receive.recv_timeout(TIMEOUT).unwrap()).unwrap();
            assert_eq!(paused["status"], "paused");
        }
        service.stop();
        reader.join().unwrap();
        let journal =
            fs::read_to_string(home.path().join("monitoring/local-activity-monitor.jsonl"))
                .unwrap();
        let events: Vec<Value> = journal
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let expected = if stop_with_restart {
            "restart_requested"
        } else {
            "launcher_closed"
        };
        assert!(events.iter().any(|event| event["kind"] == expected));
        assert_eq!(events.last().unwrap()["kind"], "stopped");
    }
}

fn bundle(path: &Path) {
    let pin: Value = serde_json::from_str(include_str!("../../../workbench-ui.json")).unwrap();
    for (folder, files) in [
        (
            "_web",
            vec![
                (
                    "index.html",
                    r#"<html><head><link rel="stylesheet" href="/style.css"><link rel="stylesheet" href="/workbench-ui.css"></head><body></body></html>"#,
                ),
                ("app.js", "console.log('</script>');"),
                ("style.css", "body{}"),
                ("locales.json", "{}"),
                ("favicon.svg", "<svg/>"),
                ("favicon.ico", "ico"),
            ],
        ),
        (
            "_workbench",
            vec![
                ("workbench-ui.js", "let wbui=1"),
                ("workbench-ui.css", ".wbui{}"),
                ("workbench-ui.mjs", ""),
                ("workbench-loader.js", ""),
            ],
        ),
    ] {
        let directory = path.join(folder);
        fs::create_dir(&directory).unwrap();
        let mut hashes = serde_json::Map::new();
        for (file, value) in files {
            let data = if file == "app.js" {
                value.repeat(100)
            } else {
                value.to_owned()
            };
            fs::write(directory.join(file), &data).unwrap();
            hashes.insert(
                file.into(),
                json!(format!("{:x}", Sha256::digest(data.as_bytes()))),
            );
        }
        let manifest = if folder == "_web" {
            json!({"version":1,"inputs":"a".repeat(64),"workbench_revision":pin["revision"],"sha256":hashes})
        } else {
            json!({"version":1,"repository":pin["repository"],"revision":pin["revision"],"sha256":hashes})
        };
        fs::write(directory.join("manifest.json"), manifest.to_string()).unwrap();
    }
}

#[test]
fn snapshot_revisions_distinguish_restart_from_asset_change() {
    let home = tempdir().unwrap();
    let assets = tempdir().unwrap();
    bundle(assets.path());
    let mut first = Service::start(home.path(), false, Some(assets.path()));
    let baseline = first.get("/api/snapshot").json();
    assert_eq!(baseline["frontend_revision"].as_str().unwrap().len(), 12);
    assert_eq!(
        baseline["backend_revision"],
        first.get("/api/activity").json()["code_revision"]
    );
    first.stop();

    let mut restarted = Service::start(home.path(), false, Some(assets.path()));
    let unchanged = restarted.get("/api/snapshot").json();
    assert_ne!(baseline["backend_revision"], unchanged["backend_revision"]);
    assert_eq!(
        baseline["frontend_revision"],
        unchanged["frontend_revision"]
    );
    restarted.stop();

    let web = assets.path().join("_web");
    let manifest = web.join("manifest.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let script = b"let changed = true;";
    fs::write(web.join("app.js"), script).unwrap();
    value["sha256"]["app.js"] = json!(format!("{:x}", Sha256::digest(script)));
    fs::write(manifest, value.to_string()).unwrap();

    let mut updated = Service::start(home.path(), false, Some(assets.path()));
    assert_ne!(
        baseline["frontend_revision"],
        updated.get("/api/snapshot").json()["frontend_revision"]
    );
    updated.stop();
}

#[test]
fn serves_only_validated_assets_and_hashes_the_delivered_inline_content() {
    let home = tempdir().unwrap();
    let assets = tempdir().unwrap();
    bundle(assets.path());
    let mut service = Service::start(home.path(), false, Some(assets.path()));
    let reply = service.request(&format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept-Encoding: gzip\r\n",
        service.port
    ));
    assert_eq!(reply.status, 200);
    assert_eq!(reply.header("content-encoding"), Some("gzip"));
    let mut html = String::new();
    GzDecoder::new(reply.body.as_slice())
        .read_to_string(&mut html)
        .unwrap();
    let script = html
        .split_once("<script>")
        .unwrap()
        .1
        .split_once("</script>")
        .unwrap()
        .0;
    let style = html
        .split_once("<style>")
        .unwrap()
        .1
        .split_once("</style>")
        .unwrap()
        .0;
    assert!(script.contains(r"<\/script>"));
    let csp = reply.header("content-security-policy").unwrap();
    for asset in [script, style] {
        let hash = STANDARD.encode(Sha256::digest(asset.as_bytes()));
        assert!(csp.contains(&format!("'sha256-{hash}'")));
    }
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    assert_eq!(service.get("/workbench-ui.js").body, b"let wbui=1");
    assert_eq!(service.get("/workbench-loader.js").status, 404);
    assert_eq!(service.get("/_web/manifest.json").status, 404);
    service.stop();

    let changed = assets.path().join("_web/app.js");
    fs::write(&changed, "unexpected change").unwrap();
    let result = Command::new(BINARY)
        .args(["--port", "0", "--codex-home"])
        .arg(home.path())
        .arg("--assets")
        .arg(assets.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Asset checksum mismatch"));
    assert_eq!(fs::read_to_string(changed).unwrap(), "unexpected change");
}

fn snapshot_when(service: &Service, accept: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let snapshot = service.get("/api/snapshot?window=all").json();
        if accept(&snapshot) {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "snapshot did not settle: {snapshot}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn schedules_follow_sqlite_events_only_when_collection_is_enabled() {
    let home = tempdir().unwrap();
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let database = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    database.execute_batch("CREATE TABLE automations(id TEXT, status TEXT, name TEXT, prompt TEXT); INSERT INTO automations VALUES('fixture','ACTIVE','First','private');").unwrap();
    let disabled = Service::start(home.path(), false, None);
    assert_eq!(
        disabled.get("/api/snapshot").json()["codex"]["schedules"]["health"],
        "disabled"
    );
    drop(disabled);
    let service = Service::start(home.path(), true, None);
    let initial = snapshot_when(&service, |value| {
        value["codex"]["schedules"]["health"] == "ok"
    });
    assert_eq!(initial["codex"]["schedules"]["items"][0]["name"], "First");
    assert!(!initial["codex"]["schedules"]
        .to_string()
        .contains("private"));
    database
        .execute(
            "UPDATE automations SET name='Second' WHERE id='fixture'",
            [],
        )
        .unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["schedules"]["items"][0]["name"] == "Second"
    });
}

#[test]
fn schedule_remote_cache_updates_from_global_state() {
    let home = tempdir().unwrap();
    let path = home.path().join(".codex-global-state.json");
    let state = |title| {
        json!({"electron-persisted-atom-state":{
            "remote-thread-summaries-v3:remote-control:fixture":[{
                "conversationId":"00000000-0000-4000-8000-000000000001",
                "title":title,"threadSource":"schedule","updatedAt":1700000000,
                "threadRuntimeStatus":{"type":"completed"},"prompt":"private"
            }]
        }})
        .to_string()
    };
    fs::write(&path, state("First")).unwrap();
    let service = Service::start(home.path(), true, None);
    let initial = snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["remote_activities"][0]["name"] == "First"
    });
    assert!(!initial["codex"]["schedules"]
        .to_string()
        .contains("private"));
    fs::write(&path, state("Second")).unwrap();
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["remote_activities"][0]["name"] == "Second"
    });
}

#[test]
fn metadata_setting_stops_and_resumes_schedule_collection() {
    let home = tempdir().unwrap();
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let db = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    db.execute_batch("CREATE TABLE automations(id TEXT, status TEXT, name TEXT); INSERT INTO automations VALUES('fixture','ACTIVE','Before');").unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["health"] == "ok"
    });
    let set = |enabled: bool| {
        let body = json!({"observations":{"metadata":enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()), &body)
    };
    assert_eq!(set(false).status, 200);
    let disabled = service.get("/api/snapshot").json();
    assert_eq!(disabled["settings"]["observations"]["metadata"], false);
    assert_eq!(
        disabled["codex"]["schedules"],
        json!({"items":[],"health":"disabled"})
    );
    assert_eq!(disabled["codex"]["metadata_sources"], json!([]));
    db.execute("UPDATE automations SET name='After'", [])
        .unwrap();
    assert_eq!(set(true).status, 200);
    let enabled = service.get("/api/snapshot").json();
    assert_eq!(enabled["settings"]["observations"]["metadata"], true);
    assert_eq!(enabled["codex"]["schedules"]["items"][0]["name"], "After");
}

#[test]
fn state_database_changes_reconcile_remote_schedule_classification() {
    let home = tempdir().unwrap();
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let catalog = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    catalog.execute_batch("CREATE TABLE automations(id TEXT, status TEXT); CREATE TABLE local_thread_catalog(host_id TEXT, thread_id TEXT, display_title TEXT, source_created_at INTEGER, source_updated_at INTEGER, source_kind TEXT, thread_source TEXT, project_id TEXT); INSERT INTO local_thread_catalog VALUES('durable','fixture-thread','Cloud',0,1700000000,'chatgpt','schedule',NULL);").unwrap();
    let state = rusqlite::Connection::open(home.path().join("state_7.sqlite")).unwrap();
    state.execute_batch("CREATE TABLE threads(id TEXT, title TEXT, thread_source TEXT); INSERT INTO threads VALUES('fixture-thread','Fixture','user');").unwrap();
    let service = Service::start(home.path(), true, None);
    let initial = snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["health"] == "ok"
    });
    assert_eq!(
        initial["codex"]["schedules"]["remote_activities"],
        json!([])
    );
    state
        .execute("UPDATE threads SET thread_source=NULL", [])
        .unwrap();
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["remote_activities"][0]["thread_id"] == "fixture-thread"
    });
}

#[test]
fn observed_schedule_name_updates_when_session_index_changes() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    fs::create_dir(home.path().join("sqlite")).unwrap();
    let catalog = rusqlite::Connection::open(home.path().join("sqlite/codex-dev.db")).unwrap();
    catalog.execute_batch("CREATE TABLE automations(id TEXT, status TEXT); CREATE TABLE local_thread_catalog(host_id TEXT, thread_id TEXT, display_title TEXT, source_created_at INTEGER, source_updated_at INTEGER, source_kind TEXT, thread_source TEXT, project_id TEXT); INSERT INTO local_thread_catalog VALUES('durable','00000000-0000-4000-8000-000000000002',NULL,0,1700000000,'chatgpt','schedule',NULL);").unwrap();
    let path = home.path().join("session_index.jsonl");
    fs::write(
        &path,
        "{\"id\":\"00000000-0000-4000-8000-000000000002\",\"thread_name\":\"First\"}\n",
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["remote_activities"][0]["name"] == "First"
    });
    fs::write(
        path,
        "{\"id\":\"00000000-0000-4000-8000-000000000002\",\"thread_name\":\"Second\"}\n",
    )
    .unwrap();
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["remote_activities"][0]["name"] == "Second"
    });
}

fn event(time: &str, kind: &str, payload: Value) -> String {
    format!(
        "{}\n",
        json!({"timestamp":time, "type":kind, "payload":payload})
    )
}

#[test]
fn native_session_notifications_publish_only_selected_incremental_metadata() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), true, None);
    let waiting = snapshot_when(&service, |value| value["codex"]["health"] == "missing");
    assert_eq!(waiting["activity"]["watch_health"], "ok");

    let root = home.path().join("sessions/2026/11/12");
    fs::create_dir_all(&root).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let file = root.join(format!("rollout-demo-{id}.jsonl"));
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let old =
        (now - chrono::Duration::days(30)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let before = [
        event(&stamp, "session_meta", json!({"id":id,"originator":"Codex Desktop","cwd":"PRIVATE_ROOT"})),
        event(&stamp, "turn_context", json!({"model":"demo-model","reasoning_effort":"medium"})),
        event(&stamp, "event_msg", json!({"type":"task_started"})),
        event(&old, "response_item", json!({"type":"function_call","name":"old_tool","call_id":"old"})),
        event(&stamp, "response_item", json!({"type":"function_call","namespace":"mcp__demo","name":"inspect","call_id":"current","arguments":"PRIVATE_ARGUMENT"})),
        event(&stamp, "response_item", json!({"type":"message","content":"PRIVATE_MESSAGE"})),
    ].concat();
    fs::write(&file, before.as_bytes()).unwrap();
    let initial = snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 2);
    assert_eq!(initial["codex"]["bytes_read"], before.len());
    assert_eq!(initial["codex"]["threads"][0]["model"], "demo-model");
    assert_eq!(initial["codex"]["threads"][0]["status"], "running");
    assert_eq!(
        initial["codex"]["tools"],
        json!({"old_tool":1,"mcp__demo__inspect":1})
    );
    assert_eq!(
        service.get("/api/snapshot?window=1h").json()["codex"]["tools"],
        json!({"mcp__demo__inspect":1})
    );

    let next = event(
        &stamp,
        "token_usage_record",
        json!({"thread_token_usage":{"input_tokens":0,"output_tokens":12,"reasoning_output_tokens":false}}),
    );
    let split = next.len() / 2;
    fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap()
        .write_all(&next.as_bytes()[..split])
        .unwrap();
    let partial = snapshot_when(&service, |value| {
        value["codex"]["bytes_read"] == before.len() + split
    });
    assert_eq!(partial["codex"]["threads"][0]["tokens"], json!({}));
    assert_eq!(partial["codex"]["malformed_lines"], 0);
    let end = [
        &next[split..],
        &event(
            &format!("{}900Z", stamp.trim_end_matches('Z')),
            "event_msg",
            json!({"type":"token_count","info":{"total_token_usage":{"input_tokens":999}}}),
        ),
        &event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output","call_id":"current","output":"PRIVATE_OUTPUT"}),
        ),
        &event(&stamp, "event_msg", json!({"type":"task_complete"})),
        "{invalid}\n",
    ]
    .concat();
    fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap()
        .write_all(end.as_bytes())
        .unwrap();
    let after = snapshot_when(&service, |value| {
        value["codex"]["threads"][0]["status"] == "completed"
    });
    assert_eq!(
        after["codex"]["bytes_read"],
        before.len() + split + end.len()
    );
    assert_eq!(after["codex"]["malformed_lines"], 1);
    let row = &after["codex"]["threads"][0];
    assert_eq!(row["tokens"]["input_tokens"], 0);
    assert_eq!(row["tokens"]["output_tokens"], 12);
    assert!(row["tokens"]["reasoning_output_tokens"].is_null());
    assert_eq!(row["tool_events"][0]["completed_at"], stamp);
    assert!(!after.to_string().contains("PRIVATE_"));

    fs::write(&file, event(&stamp, "session_meta", json!({"id":id}))).unwrap();
    let truncated = snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 0);
    assert_eq!(truncated["codex"]["threads"][0]["status"], "observed");
    assert_eq!(truncated["codex"]["threads"][0]["tokens"], json!({}));
    assert_eq!(
        truncated["codex"]["read_state"]["checkpoint"]["write_health"],
        "ok"
    );
    service.stop();
}

#[test]
fn session_sources_require_opt_in_and_an_allowlisted_window() {
    let home = tempdir().unwrap();
    let directory = home.path().join("sessions");
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("rollout-demo-00000000-0000-4000-8000-000000000001.jsonl"),
        event(
            "2026-01-01T00:00:00Z",
            "session_meta",
            json!({"id":"00000000-0000-4000-8000-000000000001"}),
        ),
    )
    .unwrap();
    let mut service = Service::start(home.path(), false, None);
    let snapshot = service.get("/api/snapshot").json();
    assert_eq!(snapshot["codex"]["health"], "disabled");
    assert_eq!(snapshot["codex"]["files"], 0);
    assert_eq!(snapshot["codex"]["bytes_read"], 0);
    assert_eq!(snapshot["codex"]["threads"], json!([]));
    assert!(!home.path().join("monitoring").exists());
    assert_eq!(
        service.get("/api/activity").json()["watch_health"],
        "disabled"
    );
    for query in ["window=bad", "window=1h&window=all", "other=1"] {
        assert_eq!(service.get(&format!("/api/snapshot?{query}")).status, 400);
    }
    service.stop();
}

#[cfg(unix)]
#[test]
fn session_discovery_does_not_follow_linked_files_or_directories() {
    use std::os::unix::fs::symlink;

    let home = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let sessions = home.path().join("sessions");
    fs::create_dir(&sessions).unwrap();
    let id = "00000000-0000-4000-8000-000000000009";
    let name = format!("rollout-demo-{id}.jsonl");
    let source = elsewhere.path().join(&name);
    fs::write(
        &source,
        event("2026-01-01T00:00:00Z", "session_meta", json!({"id":id})),
    )
    .unwrap();
    symlink(&source, sessions.join(name)).unwrap();
    symlink(elsewhere.path(), sessions.join("linked")).unwrap();

    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["codex"]["health"] == "ok");
    assert_eq!(snapshot["codex"]["files"], 0);
    assert_eq!(snapshot["codex"]["bytes_read"], 0);
    assert_eq!(snapshot["codex"]["threads"], json!([]));
    service.stop();
}

struct EventStream(BufReader<TcpStream>);

impl EventStream {
    fn connect(service: &Service, query: &str) -> Result<Self, u16> {
        let mut stream =
            TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, service.port).into(), TIMEOUT)
                .unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.set_write_timeout(Some(TIMEOUT)).unwrap();
        write!(stream, "GET /api/events{query} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept-Encoding: gzip\r\n\r\n", service.port).unwrap();
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        assert_ne!(reader.read_line(&mut status).unwrap(), 0);
        let status = status
            .split_ascii_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        if status != 200 {
            return Err(status);
        }
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            headers.push_str(&line.to_ascii_lowercase());
        }
        assert!(headers.contains("content-type: text/event-stream; charset=utf-8"));
        assert!(headers.contains("cache-control: no-store"));
        assert!(headers.contains("transfer-encoding: chunked"));
        assert!(!headers.contains("content-encoding:"));
        Ok(Self(reader))
    }

    fn frame(&mut self) -> String {
        let mut line = String::new();
        assert_ne!(
            self.0.read_line(&mut line).unwrap(),
            0,
            "stream closed before frame"
        );
        let size = usize::from_str_radix(line.trim(), 16).unwrap();
        assert!((1..=1024 * 1024).contains(&size));
        let mut bytes = vec![0; size];
        self.0.read_exact(&mut bytes).unwrap();
        let mut suffix = [0; 2];
        self.0.read_exact(&mut suffix).unwrap();
        assert_eq!(suffix, *b"\r\n");
        let message = String::from_utf8(bytes).unwrap();
        assert!(message.ends_with("\n\n"));
        message
    }

    fn snapshot(&mut self) -> Value {
        let frame = self.frame();
        let mut lines = frame.lines();
        let version = lines
            .next()
            .unwrap()
            .strip_prefix("id: ")
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert_eq!(lines.next().unwrap(), "event: snapshot");
        let data: Value =
            serde_json::from_str(lines.next().unwrap().strip_prefix("data: ").unwrap()).unwrap();
        assert_eq!(data["version"], version);
        data
    }
}

fn native_event_fixture(home: &Path) -> (std::path::PathBuf, String) {
    let sessions = home.join("sessions");
    fs::create_dir(&sessions).unwrap();
    let id = "00000000-0000-4000-8000-000000000002";
    let path = sessions.join(format!("rollout-demo-{id}.jsonl"));
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let old =
        (now - chrono::Duration::days(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let lines = [
        event(&stamp, "session_meta", json!({"id":id})),
        event(
            &old,
            "event_msg",
            json!({"type":"turn_failed","error_code":"old_failure"}),
        ),
        event(
            &stamp,
            "event_msg",
            json!({"type":"error","error":{"code":"initial_failure","message":"PRIVATE_ERROR"}}),
        ),
    ];
    fs::write(&path, lines.concat()).unwrap();
    (path, stamp)
}

#[test]
fn stream_and_http_publish_matching_window_snapshots_and_session_logs() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| snapshot["codex"]["health"] == "ok");
    let mut stream = EventStream::connect(&service, "?window=1h").ok().unwrap();
    let first = stream.snapshot();
    assert_eq!(first["window"], "1h");
    assert_eq!(
        first["snapshot"],
        service.get("/api/snapshot?window=1h").json()
    );
    assert_eq!(first["logs"], service.get("/api/logs?window=1h").json());
    assert_eq!(first["logs"]["total"], 1);
    assert_eq!(first["logs"]["entries"][0]["code"], "initial_failure");
    assert_eq!(service.get("/api/logs").json()["total"], 2);

    let line = event(
        &stamp,
        "event_msg",
        json!({"type":"turn_aborted","error_code":"cancelled","detail":"PRIVATE_ABORT"}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(line.as_bytes())
        .unwrap();
    let after = stream.snapshot();
    assert!(after["version"].as_u64().unwrap() > first["version"].as_u64().unwrap());
    assert_eq!(
        after["snapshot"],
        service.get("/api/snapshot?window=1h").json()
    );
    assert_eq!(after["logs"], service.get("/api/logs?window=1h").json());
    assert_eq!(after["logs"]["total"], 2);
    assert_eq!(
        after["snapshot"]["errors"]["events"],
        after["logs"]["entries"]
    );
    assert_eq!(
        after["snapshot"]["errors"]["categories"],
        json!({"conversation":2})
    );
    assert_eq!(
        after["snapshot"]["errors"]["severities"],
        json!({"error":1,"warning":1})
    );
    assert!(after["logs"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["code"] == "cancelled" && entry["severity"] == "warning"));
    assert!(!first.to_string().contains("PRIVATE_"));
    assert!(!after.to_string().contains("PRIVATE_"));

    let mut reconnected = EventStream::connect(&service, "?window=1h").ok().unwrap();
    assert_eq!(reconnected.snapshot(), after);
    assert_eq!(
        EventStream::connect(&service, "?window=bad").err(),
        Some(400)
    );
    assert_eq!(service.get("/api/logs?window=bad").status, 400);
    service.stop();
}

#[test]
fn stream_limit_releases_disconnected_clients_and_keeps_http_available() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    let mut clients = (0..8)
        .map(|_| {
            let mut client = EventStream::connect(&service, "").ok().unwrap();
            assert_eq!(client.snapshot()["snapshot"]["codex"]["health"], "disabled");
            client
        })
        .collect::<Vec<_>>();
    assert_eq!(EventStream::connect(&service, "").err(), Some(503));
    assert_eq!(service.get("/api/snapshot").status, 200);
    drop(clients.pop());
    let deadline = Instant::now() + TIMEOUT;
    let mut recovered = loop {
        match EventStream::connect(&service, "") {
            Ok(stream) => break stream,
            Err(503) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(status) => panic!("stream permit was not released: {status}"),
        }
    };
    assert_eq!(recovered.snapshot()["logs"]["enabled"], false);
    service.stop();
}

#[test]
fn idle_stream_heartbeats_do_not_republish_the_source() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| snapshot["codex"]["health"] == "ok");
    let mut stream = EventStream::connect(&service, "").ok().unwrap();
    let first = stream.snapshot();
    fs::write(home.path().join("sessions/ignored.txt"), "unrelated").unwrap();
    stream
        .0
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(18)))
        .unwrap();
    assert_eq!(stream.frame(), ": heartbeat\n\n");
    assert_eq!(service.get("/api/snapshot").json(), first["snapshot"]);
    service.stop();
}

fn checkpoint_key(path: &Path) -> String {
    let relative = path.file_name().unwrap().to_str().unwrap();
    format!("rust:v1:{:x}", Sha256::digest(relative.as_bytes()))
}

#[test]
fn checkpoint_resumes_completed_records_and_preserves_python_rows() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    let key = checkpoint_key(&path);
    let legacy_key = key.strip_prefix("rust:v1:").unwrap().to_owned();
    {
        let mut storage = Store::open(&monitoring, Access::ReadWrite)
            .unwrap()
            .unwrap();
        storage
            .save_checkpoints(
                &[lam_history_store::Cursor {
                    source_key: legacy_key.clone(),
                    offset: 1,
                    signature: "python signature".into(),
                    payload: r#"{"version":1,"state":{"thread_id":"legacy-example"}}"#.into(),
                }],
                &[],
                &[],
            )
            .unwrap();
    }
    let mut complete = fs::read(&path).unwrap();
    complete.extend(event(&stamp, "event_msg", json!({"type":"task_started"})).as_bytes());
    complete.extend(event(&stamp, "response_item", json!({"type":"function_call","name":"first_tool","call_id":"first","arguments":"PRIVATE_REQUEST"})).as_bytes());
    let partial = event(
        &stamp,
        "token_usage_record",
        json!({"thread_token_usage":{"input_tokens":0}}),
    );
    let split = partial.len() / 2;
    fs::write(
        &path,
        [&complete[..], &partial.as_bytes()[..split]].concat(),
    )
    .unwrap();
    {
        let mut service = Service::start(home.path(), true, None);
        let first = snapshot_when(&service, |value| {
            value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
        });
        assert_eq!(first["codex"]["bytes_read"], complete.len() + split);
        assert_eq!(first["codex"]["observed_tool_calls"], 1);
        assert_eq!(first["codex"]["threads"][0]["tokens"], json!({}));
        let mut storage = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
        let saved = storage.checkpoint(&key, 10).unwrap().unwrap();
        assert_eq!(saved.cursor.offset, complete.len() as i64);
        assert!(!saved.cursor.payload.contains("PRIVATE_"));
        assert!(!saved.calls[0].payload.contains("PRIVATE_"));
        assert_eq!(
            storage
                .checkpoint(&legacy_key, 10)
                .unwrap()
                .unwrap()
                .cursor
                .offset,
            1
        );
        service.stop();
    }
    let ending = [
        partial[split..].to_owned(),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output","call_id":"first","output":"PRIVATE_OUTPUT"}),
        ),
        event(&stamp, "event_msg", json!({"type":"task_complete"})),
    ]
    .concat();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(ending.as_bytes())
        .unwrap();
    {
        let mut service = Service::start(home.path(), true, None);
        let resumed = snapshot_when(&service, |value| {
            value["codex"]["threads"][0]["status"] == "completed"
        });
        assert_eq!(resumed["codex"]["read_state"]["checkpoint"]["restored"], 1);
        assert_eq!(resumed["codex"]["bytes_read"], split + ending.len());
        assert_eq!(resumed["codex"]["observed_tool_calls"], 1);
        assert_eq!(resumed["codex"]["threads"][0]["tokens"]["input_tokens"], 0);
        assert_eq!(
            resumed["codex"]["threads"][0]["tool_events"][0]["completed_at"],
            stamp
        );
        assert_eq!(resumed["errors"]["total"], 2);
        service.stop();
    }
    let mut service = Service::start(home.path(), true, None);
    let unchanged = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(unchanged["codex"]["bytes_read"], 0);
    assert_eq!(unchanged["codex"]["threads"][0]["status"], "completed");
    service.stop();
}

#[test]
fn replacement_and_prefix_rewrite_invalidate_checkpoint_calls() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    let key = checkpoint_key(&path);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"old_tool","call_id":"old"}),
            )
            .as_bytes(),
        )
        .unwrap();
    {
        let mut service = Service::start(home.path(), true, None);
        let first = snapshot_when(&service, |value| {
            value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
        });
        assert_eq!(first["codex"]["tools"], json!({"old_tool":1}));
        service.stop();
    }
    fs::rename(&path, path.with_extension("old")).unwrap();
    let id = "00000000-0000-4000-8000-000000000002";
    let replaced = [
        event(
            &stamp,
            "session_meta",
            json!({"id":id,"originator":"Codex Desktop"}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"replacement","call_id":"new"}),
        ),
    ]
    .concat();
    fs::write(&path, &replaced).unwrap();
    let mut service = Service::start(home.path(), true, None);
    let fallback = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    assert_eq!(
        fallback["codex"]["read_state"]["checkpoint"]["load_health"],
        "stale"
    );
    assert_eq!(fallback["codex"]["read_state"]["checkpoint"]["restored"], 0);
    assert_eq!(fallback["codex"]["bytes_read"], replaced.len());
    assert_eq!(fallback["codex"]["tools"], json!({"replacement":1}));

    let rewritten = [
        event(&stamp, "session_meta", json!({"id":id,"originator":"codex_work_desktop"})),
        event(&stamp, "response_item", json!({"type":"function_call","name":"rewritten","call_id":"latest","arguments":"x".repeat(4096)})),
    ].concat();
    assert!(rewritten.len() > replaced.len());
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .write_all(rewritten.as_bytes())
        .unwrap();
    let live = snapshot_when(&service, |value| {
        value["codex"]["tools"] == json!({"rewritten":1})
    });
    assert_eq!(live["codex"]["read_state"]["checkpoint"]["invalidated"], 2);
    assert_eq!(
        live["codex"]["bytes_read"],
        replaced.len() + rewritten.len()
    );
    let mut storage = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
    let saved = storage.checkpoint(&key, 10).unwrap().unwrap();
    assert_eq!(saved.calls.len(), 1);
    assert_eq!(saved.calls[0].call_id, "latest");
    service.stop();
}

#[test]
fn checkpoint_retries_uncommitted_calls_after_writer_lock() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    let key = checkpoint_key(&path);
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let original_size = fs::metadata(&path).unwrap().len() as i64;

    let writer = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let first = event(
        &stamp,
        "response_item",
        json!({"type":"function_call","name":"first","call_id":"first"}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(first.as_bytes())
        .unwrap();
    let unavailable = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "unavailable"
    });
    assert_eq!(unavailable["codex"]["tools"], json!({"first":1}));
    {
        let mut storage = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
        let saved = storage.checkpoint(&key, 10).unwrap().unwrap();
        assert_eq!(saved.cursor.offset, original_size);
        assert!(saved.calls.is_empty());
    }
    writer.execute_batch("ROLLBACK").unwrap();

    let second = event(
        &stamp,
        "response_item",
        json!({"type":"function_call","name":"second","call_id":"second"}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(second.as_bytes())
        .unwrap();
    let recovered = snapshot_when(&service, |value| {
        value["codex"]["observed_tool_calls"] == 2
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    assert_eq!(recovered["codex"]["tools"], json!({"first":1,"second":1}));
    let mut storage = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
    let saved = storage.checkpoint(&key, 10).unwrap().unwrap();
    assert_eq!(
        saved.cursor.offset,
        original_size + first.len() as i64 + second.len() as i64
    );
    assert_eq!(
        saved
            .calls
            .into_iter()
            .map(|call| call.call_id)
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    service.stop();
}

#[test]
fn concurrent_native_collectors_do_not_overwrite_a_committed_checkpoint() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    {
        let mut seed = Service::start(home.path(), true, None);
        snapshot_when(&seed, |value| {
            value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
        });
        seed.stop();
    }
    let mut first = Service::start(home.path(), true, None);
    let mut second = Service::start(home.path(), true, None);
    for service in [&first, &second] {
        snapshot_when(service, |value| {
            value["codex"]["read_state"]["checkpoint"]["restored"] == 1
        });
    }

    let appended = event(
        &stamp,
        "response_item",
        json!({"type":"function_call","name":"shared","call_id":"shared"}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(appended.as_bytes())
        .unwrap();
    let states = [&first, &second].map(|service| {
        snapshot_when(service, |value| {
            value["codex"]["tools"] == json!({"shared":1})
        })
    });
    let mut status = states.each_ref().map(|state| {
        state["codex"]["read_state"]["checkpoint"]["write_health"]
            .as_str()
            .unwrap()
    });
    status.sort_unstable();
    assert_eq!(status, ["conflict", "ok"]);

    let mut storage = Store::open(&home.path().join("monitoring"), Access::ReadOnly)
        .unwrap()
        .unwrap();
    let saved = storage
        .checkpoint(&checkpoint_key(&path), 10)
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.cursor.offset,
        fs::metadata(&path).unwrap().len() as i64
    );
    assert_eq!(saved.calls.len(), 1);
    assert_eq!(saved.calls[0].call_id, "shared");
    assert_eq!(
        serde_json::from_str::<Value>(&saved.cursor.signature).unwrap()["revision"],
        2
    );
    first.stop();
    second.stop();
}

#[test]
fn rust_startup_upgrades_legacy_history_before_serving_read_only_pages() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    fs::create_dir(&monitoring).unwrap();
    let database = monitoring.join(lam_history_store::FILE_NAME);
    {
        let old = rusqlite::Connection::open(&database).unwrap();
        old.execute_batch(
            "CREATE TABLE history_items (namespace TEXT NOT NULL, section TEXT NOT NULL,
             item_key TEXT NOT NULL, ordinal INTEGER NOT NULL, event_time TEXT, payload TEXT NOT NULL,
             PRIMARY KEY(namespace,section,item_key));
             CREATE TABLE history_state(namespace TEXT PRIMARY KEY,metadata TEXT NOT NULL,updated_at INTEGER NOT NULL);
             CREATE TABLE session_cursors(source_key TEXT PRIMARY KEY,offset INTEGER NOT NULL,signature TEXT NOT NULL,payload TEXT NOT NULL);
             CREATE TABLE session_calls(source_key TEXT NOT NULL,call_id TEXT NOT NULL,event_time TEXT,payload TEXT NOT NULL,PRIMARY KEY(source_key,call_id));",
        ).unwrap();
        old.pragma_update(None, "application_id", lam_history_store::APPLICATION_ID)
            .unwrap();
        old.pragma_update(None, "user_version", 3).unwrap();
        old.execute(
            "INSERT INTO history_items VALUES('activity','mcp','legacy-row',0,NULL,?)",
            [r#"{"server":"demo","call_id":"legacy-row","nested":true}"#],
        )
        .unwrap();
    }
    let mut service = Service::start(home.path(), false, None);
    let page = service.get("/api/history?section=mcp").json();
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["call_id"], "legacy-row");
    assert_eq!(
        service.get("/api/snapshot").json()["codex"]["health"],
        "disabled"
    );
    let saved = rusqlite::Connection::open(
        monitoring.join(format!("{}.schema-v3.bak", lam_history_store::FILE_NAME)),
    )
    .unwrap();
    assert_eq!(
        saved
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    let current = rusqlite::Connection::open(database).unwrap();
    assert_eq!(
        current
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        4
    );
    service.stop();
}

#[test]
fn rust_startup_imports_legacy_history_and_respects_session_opt_in() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    fs::create_dir(&monitoring).unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let activity = monitoring.join("activity-history.json");
    let threads = monitoring.join("thread-state.json");
    let invalid = monitoring.join("error-history.json");
    fs::write(&activity, json!({"version":1,"sql":[],"web":[
        {"timestamp":stamp,"server":"web","tool":"first","call_id":"first","payload":"PRIVATE_INPUT"}
    ]}).to_string()).unwrap();
    fs::write(&threads, json!({"version":1,"entries":{
        "rollout-example.jsonl":{"thread_id":"00000000-0000-4000-8000-000000000002","status":"completed","task_time":stamp,"offset":17}
    }}).to_string()).unwrap();
    fs::write(&invalid, b"invalid source").unwrap();

    let first_page = {
        let mut service = Service::start(home.path(), false, None);
        let page = service.get("/api/history?section=web").json();
        assert_eq!(page["total"], 1);
        assert_eq!(page["items"][0]["tool"], "first");
        assert!(!page.to_string().contains("PRIVATE_INPUT"));
        assert_eq!(
            service
                .get("/api/history?namespace=threads&section=entries")
                .status,
            400
        );
        assert_eq!(
            service
                .get("/api/history?namespace=errors&section=events")
                .json()["total"],
            0
        );
        service.stop();
        page
    };
    let mut reader = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
    assert_eq!(
        reader
            .page(PageQuery {
                section: Section::Threads,
                limit: 20,
                offset: 0,
                since: None,
                after: None,
                excluded_sources: &[],
            })
            .unwrap()
            .total,
        0
    );
    drop(reader);

    fs::write(
        &activity,
        json!({"version":1,"sql":[],"web":[
            {"timestamp":stamp,"server":"web","tool":"later","call_id":"later"}
        ]})
        .to_string(),
    )
    .unwrap();
    let mut service = Service::start(home.path(), true, None);
    assert_eq!(service.get("/api/history?section=web").json(), first_page);
    let imported = service
        .get("/api/history?namespace=threads&section=entries")
        .json();
    assert_eq!(imported["total"], 1);
    assert_eq!(imported["items"][0]["offset"], 17);
    assert_eq!(fs::read(invalid).unwrap(), b"invalid source");
    service.stop();
}

fn append_records(path: &Path, records: &[String]) {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(records.concat().as_bytes())
        .unwrap();
}

#[test]
fn native_mcp_and_web_events_reach_history_and_survive_session_removal() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","namespace":"mcp__local_documents",
            "name":"read","call_id":"mcp","arguments":r#"{"source":"PRIVATE_SOURCE.pdf","page_id":"page-1","runId":"sk-PRIVATE","ocr":"auto","write":false,"body":"PRIVATE_ARGUMENT"}"#}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"web.run","call_id":"web",
            "arguments":{"open":[{"ref_id":"https://example.com/docs?key=PRIVATE_LINK#part"},{"ref_id":"http://127.0.0.1/PRIVATE_HOST"}]}}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    let pending = snapshot_when(&service, |value| value["mcp"]["total_events"] == 2);
    let pending_mcp = pending["mcp"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["server"] == "local_documents")
        .unwrap();
    assert_eq!(pending_mcp["metadata"]["format"], "pdf");
    assert_eq!(pending_mcp["metadata"]["ocr_mode"], "auto");
    assert_eq!(pending_mcp["metadata"]["write"], false);
    assert_eq!(
        pending_mcp["metadata"]["resources"],
        json!({"page_id":"page-1"})
    );
    let pending_server = pending["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["server"] == "local_documents")
        .unwrap();
    assert_eq!(pending_server["connection"]["state"], "pending");
    assert_eq!(pending_server["calls"], 1);
    assert_eq!(service.get("/api/history?section=mcp").json()["total"], 1);
    assert_eq!(service.get("/api/history?section=web").json()["total"], 1);

    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"mcp",
            "output":{"structuredContent":{"status":"ok","input_tokens":0,"cache_hit":false,
                                          "secret_count":99,"content":"PRIVATE_MCP_OUTPUT"}}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"web",
            "output":"Found https://example.org/page?secret=PRIVATE_WEB_OUTPUT#hash and http://127.0.0.1/PRIVATE_LOCAL"}),
            ),
        ],
    );
    let completed = snapshot_when(&service, |value| {
        value["mcp"]["servers"]
            .as_array()
            .is_some_and(|rows| rows.len() == 2 && rows.iter().all(|row| row["returned"] == 1))
    });
    let events = completed["mcp"]["events"].as_array().unwrap();
    let mcp = events
        .iter()
        .find(|event| event["server"] == "local_documents")
        .unwrap();
    let web = events
        .iter()
        .find(|event| event["server"] == "web")
        .unwrap();
    assert_eq!(
        mcp["result"],
        json!({"status":"ok","input_tokens":0,"cache_hit":false})
    );
    assert_eq!(
        web["metadata"]["references"],
        json!(["https://example.com/docs"])
    );
    assert_eq!(
        web["result"]["references"],
        json!(["https://example.org/page"])
    );
    assert!(!completed.to_string().contains("PRIVATE_"));
    let history = service.get("/api/history?section=mcp").json();
    assert_eq!(history["total"], 1);
    assert_eq!(history["items"][0]["result"], mcp["result"]);
    service.stop();

    let mut service = Service::start(home.path(), true, None);
    let restored = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(restored["codex"]["bytes_read"], 0);
    assert_eq!(restored["mcp"]["total_events"], 2);
    assert!(restored["mcp"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["metadata"]["format"] == "pdf"));
    fs::remove_file(path).unwrap();
    let retained = snapshot_when(&service, |value| value["codex"]["files"] == 0);
    assert_eq!(retained["mcp"]["total_events"], 2);
    assert_eq!(retained["mcp"]["history_health"], "ok");
    service.stop();
}

#[test]
fn observation_failure_rolls_back_the_checkpoint_and_retries_on_source_update() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    let key = checkpoint_key(&path);
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let original = fs::metadata(&path).unwrap().len() as i64;
    let writer = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    writer.execute_batch("CREATE TRIGGER reject_activity BEFORE INSERT ON history_items
                          WHEN NEW.section='mcp' BEGIN SELECT RAISE(ABORT,'fixture rejected activity'); END").unwrap();
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"mcp__fixture__read","call_id":"mcp","arguments":{}}),
        )],
    );
    let failed = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "unavailable"
    });
    assert_eq!(failed["mcp"]["total_events"], 1);
    {
        let mut reader = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
        let saved = reader.checkpoint(&key, 10).unwrap().unwrap();
        assert_eq!(saved.cursor.offset, original);
        assert!(saved.calls.is_empty());
    }
    assert_eq!(service.get("/api/history?section=mcp").json()["total"], 0);
    writer
        .execute_batch("DROP TRIGGER reject_activity")
        .unwrap();
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output","call_id":"mcp","output":{"status":"ok","input_tokens":0}}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let history = service.get("/api/history?section=mcp").json();
    assert_eq!(history["total"], 1);
    assert_eq!(history["items"][0]["result"]["input_tokens"], 0);
    let mut reader = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
    assert_eq!(
        reader.checkpoint(&key, 10).unwrap().unwrap().cursor.offset,
        fs::metadata(&path).unwrap().len() as i64
    );
    service.stop();
}

#[test]
fn mcp_source_remains_visible_when_the_selected_window_has_no_calls() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let old =
        (now - chrono::Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    append_records(
        &path,
        &[
            event(
                &old,
                "response_item",
                json!({"type":"function_call","name":"mcp__old__read","call_id":"old-mcp","arguments":{}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"web.run","call_id":"current-web","arguments":{}}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| snapshot["mcp"]["total_events"] == 2);
    let recent = service.get("/api/snapshot?window=1h").json();
    assert_eq!(recent["mcp"]["total_events"], 1);
    let source = recent["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["server"] == "old")
        .unwrap();
    assert_eq!(source["calls"], 0);
    assert_eq!(source["connection"]["state"], "pending");
    service.stop();
}

#[test]
fn observed_history_applies_default_retention_and_preserves_unlimited_retention() {
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let old =
        (now - chrono::Duration::days(120)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    for (unlimited, expected) in [(false, 0), (true, 1)] {
        let home = tempdir().unwrap();
        let (path, _) = native_event_fixture(home.path());
        if unlimited {
            let monitoring = home.path().join("monitoring");
            fs::create_dir(&monitoring).unwrap();
            fs::write(
                monitoring.join("activity-history.json"),
                r#"{"version":3,"retention_days":0,"sql":[],"web":[]}"#,
            )
            .unwrap();
        }
        append_records(
            &path,
            &[event(
                &old,
                "response_item",
                json!({"type":"function_call","name":"mcp__fixture__read","call_id":"historical","arguments":{}}),
            )],
        );
        let mut service = Service::start(home.path(), true, None);
        snapshot_when(&service, |value| value["mcp"]["total_events"] == 1);
        assert_eq!(
            service.get("/api/history?section=mcp").json()["total"],
            expected
        );
        service.stop();
    }
}

#[test]
fn nested_exec_observations_keep_indexes_durations_and_result_ownership() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let completed = (lam_history_store::parse_utc_stamp(&stamp).unwrap()
        + chrono::Duration::milliseconds(2500))
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let multiple = r#"
        // tools.mcp__ghost__read({})
        const text = 'tools.mcp__ghost__read({})';
        await tools.exec_command({cmd:'PRIVATE_COMMAND'});
        await tools.mcp__local_documents__inspect({source:'PRIVATE_SOURCE.pdf',page_id:'fixture-page',ocr:'force',write:false});
        await tools.web__run({open:[{ref_id:'https://example.org/page?secret=PRIVATE_QUERY'}]});
        await tools.mcp__local_documents__inspect(runtime_args);
    "#;
    let isolated = r#"await tools.mcp__fixture__read({page_id:'isolated-page'});"#;
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call","name":"exec","namespace":"functions","call_id":"multiple","input":multiple}),
            ),
            event(
                &completed,
                "response_item",
                json!({"type":"custom_tool_call_output","call_id":"multiple",
            "output":{"status":"ok","input_tokens":42,"content":"PRIVATE_MULTI"}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call","name":"functions.exec","call_id":"isolated","input":isolated}),
            ),
            event(
                &completed,
                "response_item",
                json!({"type":"custom_tool_call_output","call_id":"isolated",
            "output":{"structuredContent":{"status":"ok","input_tokens":0,"cache_hit":false,"content":"PRIVATE_ISOLATED"}}}),
            ),
        ],
    );
    let check = |snapshot: &Value| {
        assert_eq!(snapshot["codex"]["tools"], json!({"functions.exec":2}));
        assert_eq!(
            snapshot["codex"]["nested_tools"],
            json!({
                "exec_command":1,"mcp__local_documents__inspect":2,"web__run":1,"mcp__fixture__read":1
            })
        );
        let events = snapshot["mcp"]["events"].as_array().unwrap();
        let mut multi = events
            .iter()
            .filter(|event| event["call_id"] == "multiple")
            .collect::<Vec<_>>();
        multi.sort_by_key(|event| event["index"].as_u64().unwrap());
        assert_eq!(multi.len(), 3);
        for (index, event) in multi.iter().enumerate() {
            assert_eq!(event["index"], index);
            assert_eq!(event["nested"], true);
            assert!(event["duration_ms"].is_null());
            assert_eq!(event["container_duration_ms"], 2500);
            let empty = if event["server"] == "web" {
                json!({"references":[]})
            } else {
                json!({})
            };
            assert_eq!(event["result"], empty);
        }
        assert_eq!(multi[0]["metadata"]["format"], "pdf");
        assert_eq!(multi[0]["metadata"]["write"], false);
        assert_eq!(
            multi[1]["metadata"]["references"],
            json!(["https://example.org/page"])
        );
        assert_eq!(multi[2]["metadata"]["resources"], json!({}));
        let isolated = events
            .iter()
            .find(|event| event["call_id"] == "isolated")
            .unwrap();
        assert_eq!(isolated["nested"], true);
        assert_eq!(isolated["index"], 0);
        assert_eq!(isolated["container_duration_ms"], 2500);
        assert!(isolated["duration_ms"].is_null());
        assert_eq!(
            isolated["result"],
            json!({"status":"ok","input_tokens":0,"cache_hit":false})
        );
        assert!(!snapshot.to_string().contains("PRIVATE_"));
        assert!(!snapshot.to_string().contains("mcp__ghost"));
    };
    {
        let mut service = Service::start(home.path(), true, None);
        let snapshot = snapshot_when(&service, |snapshot| snapshot["mcp"]["total_events"] == 4);
        check(&snapshot);
        assert_eq!(service.get("/api/history?section=mcp").json()["total"], 3);
        assert_eq!(service.get("/api/history?section=web").json()["total"], 1);
        service.stop();
    }
    let mut service = Service::start(home.path(), true, None);
    let restored = snapshot_when(&service, |snapshot| {
        snapshot["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(restored["codex"]["bytes_read"], 0);
    check(&restored);
    service.stop();
}

#[test]
fn nested_mcp_event_limit_preserves_the_observed_tool_count() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let code = "tools.mcp__fixture__read({page_id:'page'});".repeat(101);
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call","name":"exec","call_id":"bulk","input":code}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call_output","call_id":"bulk","output":{"status":"ok"}}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |snapshot| snapshot["mcp"]["total_events"] == 100);
    assert_eq!(snapshot["codex"]["nested_tools"]["mcp__fixture__read"], 101);
    let mut indexes = snapshot["mcp"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| {
            assert_eq!(event["result"], json!({}));
            event["index"].as_u64().unwrap()
        })
        .collect::<Vec<_>>();
    indexes.sort_unstable();
    assert_eq!(indexes, (0..100).collect::<Vec<_>>());
    assert_eq!(service.get("/api/history?section=mcp").json()["total"], 100);
    service.stop();
}

#[test]
fn git_and_check_snapshots_keep_classification_and_outer_duration() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let parsed = lam_history_store::parse_utc_stamp(&stamp).unwrap();
    let completed = (parsed + chrono::Duration::seconds(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let old =
        (parsed - chrono::Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let nested = r#"
        // tools.exec_command({cmd:"git push"});
        const text = 'tools.exec_command({cmd:"git clean"})';
        await tools.exec_command({cmd:"git status --short; git -C D:\\repo diff; python -m unittest PRIVATE_TEST",workdir:"D:\\PRIVATE_PARENT\\repo"});
    "#;
    let duplicate = event(
        &stamp,
        "response_item",
        json!({
            "type":"custom_tool_call","name":"functions.exec","call_id":"nested","input":nested
        }),
    );
    append_records(
        &path,
        &[
            duplicate.clone(),
            duplicate,
            event(
                &completed,
                "response_item",
                json!({
                    "type":"custom_tool_call_output","call_id":"nested","output":"PRIVATE_OUTPUT"
                }),
            ),
            event(
                &old,
                "response_item",
                json!({
                    "type":"function_call","name":"exec_command","call_id":"direct",
                    "arguments":{"cmd":r#"& 'C:\runtime folder\python.exe' -X utf8 -I -B -m pytest PRIVATE_TEST; cargo clippy --all-targets; git log --oneline"#,"workdir":"/PRIVATE_PARENT/project"}
                }),
            ),
        ],
    );
    let check = |all: &Value| {
        assert_eq!(
            all["codex"]["git"]["operations"],
            json!({"status":1,"diff":1,"log":1})
        );
        assert_eq!(all["codex"]["git"]["total"], 3);
        let events = all["codex"]["git"]["events"].as_array().unwrap();
        assert_eq!(events[0]["operation"], "status");
        assert_eq!(events[1]["operation"], "diff");
        assert_eq!(events[0]["repository"], "repo");
        assert_eq!(events[1]["index"], 1);
        assert_eq!(events[0]["duration_ms"], 2000);
        assert_eq!(events[2]["repository"], "project");
        let checks = all["codex"]["checks"].as_array().unwrap();
        assert_eq!(
            checks
                .iter()
                .map(|event| event["operation"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["python -m unittest", "python -m pytest", "cargo clippy"]
        );
        assert_eq!(checks[0]["category"], "python");
        assert_eq!(checks[2]["category"], "runtime");
        assert_eq!(checks[0]["duration_ms"], 2000);
        assert!(checks[1]["duration_ms"].is_null());
        assert!(!all.to_string().contains("PRIVATE_"));
    };
    {
        let mut service = Service::start(home.path(), true, None);
        let all = snapshot_when(&service, |snapshot| snapshot["codex"]["git"]["total"] == 3);
        check(&all);
        let recent = service.get("/api/snapshot?window=1h").json();
        assert_eq!(recent["codex"]["git"]["total"], 2);
        assert_eq!(recent["codex"]["checks"].as_array().unwrap().len(), 1);
        service.stop();
    }
    let mut service = Service::start(home.path(), true, None);
    let restored = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(restored["codex"]["bytes_read"], 0);
    check(&restored);
    service.stop();
}

#[test]
fn git_and_check_windows_preserve_totals_and_bound_visible_events() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let cmd = "git status;".repeat(80) + &"cargo check;".repeat(40);
    let records: Vec<_> = (0..13)
        .map(|index| {
            event(
                &stamp,
                "response_item",
                json!({
                    "type":"function_call","name":"exec_command","call_id":format!("bulk-{index}"),
                    "arguments":{"cmd":cmd},
                }),
            )
        })
        .collect();
    append_records(&path, &records);

    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["codex"]["git"]["total"] == 1040);
    assert_eq!(snapshot["codex"]["git"]["operations"]["status"], 1040);
    assert_eq!(
        snapshot["codex"]["git"]["events"].as_array().unwrap().len(),
        500
    );
    assert_eq!(snapshot["codex"]["checks"].as_array().unwrap().len(), 500);
    assert_eq!(snapshot["codex"]["read_state"]["git_event_limit"], 500);
    assert_eq!(snapshot["codex"]["read_state"]["check_event_limit"], 500);
    service.stop();
}

#[test]
fn sqlite_session_observations_keep_metadata_only_and_survive_restart() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let completed = (lam_history_store::parse_utc_stamp(&stamp).unwrap()
        + chrono::Duration::seconds(2))
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let never_opened = home.path().join("never-opened.sqlite3");
    let cli = format!(
        "sqlite3 -readonly \"{}\" \"SELECT 'PRIVATE_CLI'; PRAGMA query_only=ON\"",
        never_opened.display()
    );
    let python = "import sqlite3 as sql\nconn=sql.connect('fixture.db')\ncur=conn.cursor()\ncur.execute('SELECT PRIVATE_PYTHON')\ncur.execute(dynamic)\nconn.commit()\nconn.close()";
    let nested = format!(
        "await tools.exec_command({});",
        json!({"cmd":format!("@'\n{python}\n'@ | python -")})
    );
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"mcp__sqlite__query","call_id":"mcp-sql",
            "arguments":{"database":"file:demo.db?password=PRIVATE_URI","sql":"WITH a AS (SELECT 1) UPDATE demo SET v='PRIVATE_MCP'; SELECT 1"}}),
            ),
            event(
                &completed,
                "response_item",
                json!({"type":"function_call_output","call_id":"mcp-sql","output":{"status":"ok"}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"exec_command","call_id":"cli-sql","arguments":{"cmd":cli}}),
            ),
            event(
                &completed,
                "response_item",
                json!({"type":"function_call_output","call_id":"cli-sql","output":{"exit_code":1,"message":"PRIVATE_ERROR"}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call","name":"functions.exec","call_id":"nested-sql","input":nested}),
            ),
            event(
                &completed,
                "response_item",
                json!({"type":"custom_tool_call_output","call_id":"nested-sql","output":"Process exited with code 0"}),
            ),
        ],
    );
    let check = |snapshot: &Value| {
        let sqlite = &snapshot["codex"]["sqlite"];
        assert_eq!(sqlite["total"], 9);
        assert_eq!(
            sqlite["operations"],
            json!({"open":2,"read":3,"write":1,"pragma":1,"transaction":1,"close":1})
        );
        let events = sqlite["events"].as_array().unwrap();
        assert!(events
            .iter()
            .all(|row| row["id"].as_str().unwrap().len() == 64));
        assert!(events
            .iter()
            .all(|row| row["duration_ms"].is_null() && row["container_duration_ms"] == 2000));
        assert_eq!(
            events
                .iter()
                .filter(|row| row["call_id"] == "cli-sql" && row["result"] == "failed")
                .count(),
            3
        );
        assert_eq!(
            events
                .iter()
                .filter(|row| row["call_id"] == "nested-sql"
                    && row["nested"] == true
                    && row["recognition"] == "recorded_code")
                .count(),
            4
        );
        assert_eq!(
            events
                .iter()
                .filter(|row| row["call_id"] == "mcp-sql"
                    && row["database"] == "file:demo.db"
                    && row["result"] == "returned")
                .count(),
            2
        );
        assert_eq!(snapshot["mcp"]["total_events"], 1);
        assert!(!snapshot.to_string().contains("PRIVATE_"));
    };
    {
        let mut service = Service::start(home.path(), true, None);
        let snapshot = snapshot_when(&service, |value| value["codex"]["sqlite"]["total"] == 9);
        check(&snapshot);
        let history = service.get("/api/history?section=sql").json();
        assert_eq!(history["total"], 9);
        assert!(!history.to_string().contains("PRIVATE_"));
        service.stop();
    }
    assert!(!never_opened.exists());
    let mut service = Service::start(home.path(), true, None);
    let restored = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(restored["codex"]["bytes_read"], 0);
    check(&restored);
    fs::remove_file(path).unwrap();
    let retained = snapshot_when(&service, |value| value["codex"]["files"] == 0);
    assert_eq!(retained["codex"]["sqlite"]["total"], 9);
    service.stop();
}

#[test]
fn sqlite_and_mcp_aggregates_count_all_retained_rows_beyond_the_event_preview() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let recent =
        (now - chrono::Duration::minutes(20)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let old =
        (now - chrono::Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut writer = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    let mut raw = Vec::new();
    for (section, size) in [(Section::Sql, 501), (Section::Mcp, 1002), (Section::Web, 1)] {
        for index in 0..size {
            let time = if index == 0 && section != Section::Web {
                &old
            } else {
                &recent
            };
            let key = format!("{section:?}-{index}");
            let payload = match section {
                Section::Sql => json!({
                    "timestamp":time,"call_id":key,"engine":"SQLite","recognition":"recorded_code",
                    "statement":"SELECT","operation":"read",
                }),
                Section::Mcp => json!({
                    "timestamp":time,"call_id":key,"server":"fixture","tool":"read","nested":index % 2 == 0,
                }),
                Section::Web => {
                    json!({"timestamp":time,"call_id":key,"server":"web","tool":"run","nested":false})
                }
                _ => unreachable!(),
            };
            raw.push((section, key, time.as_str(), payload.to_string()));
        }
    }
    let rows = raw
        .iter()
        .enumerate()
        .map(|(ordinal, (section, key, time, payload))| HistoryWrite {
            section: *section,
            key,
            ordinal: ordinal as i64,
            event_time: Some(time),
            payload,
        })
        .collect::<Vec<_>>();
    writer
        .save_history(
            Namespace::Activity,
            r#"{"version":3,"retention_days":90}"#,
            1,
            None,
            &rows,
        )
        .unwrap();
    drop(writer);

    let mut service = Service::start(home.path(), true, None);
    let all = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["aggregate"]["total"] == 501
    });
    let sql = &all["codex"]["sqlite"];
    assert_eq!(sql["events"].as_array().unwrap().len(), 500);
    assert_eq!(sql["aggregate"]["scope"], "retained_database");
    assert_eq!(sql["aggregate"]["window"], "all");
    assert_eq!(sql["aggregate"]["operations"], json!({"read":501}));
    assert_eq!(sql["aggregate"]["statements"], json!({"SELECT":501}));
    let mcp = &all["mcp"]["history_aggregate"];
    assert_eq!(all["mcp"]["events"].as_array().unwrap().len(), 1000);
    assert_eq!(mcp["total"], 1002);
    assert_eq!(
        mcp["servers"],
        json!({"fixture":{"total":1002,"direct":501,"nested":501}})
    );
    let series = sql["aggregate"]["series"].as_array().unwrap();
    assert_eq!(
        series
            .iter()
            .map(|bucket| bucket["calls"].as_i64().unwrap())
            .sum::<i64>(),
        501
    );
    assert!(series
        .iter()
        .all(|bucket| bucket["time"].as_str().unwrap().ends_with('Z')));
    let recent = service.get("/api/snapshot?window=1h").json();
    assert_eq!(recent["codex"]["sqlite"]["aggregate"]["total"], 500);
    assert_eq!(recent["mcp"]["history_aggregate"]["total"], 1001);
    assert_eq!(recent["mcp"]["history_aggregate"]["window"], "1h");
    service.stop();

    let mut disabled = Service::start(home.path(), false, None);
    let disabled_snapshot = disabled.get("/api/snapshot?window=all").json();
    assert!(disabled_snapshot["codex"]["sqlite"]
        .get("aggregate")
        .is_none());
    assert!(disabled_snapshot["mcp"].get("history_aggregate").is_none());
    disabled.stop();
}

#[test]
fn unreadable_aggregate_does_not_claim_full_history_coverage() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut writer = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    writer
        .save_history(Namespace::Activity, "invalid metadata", 1, None, &[])
        .unwrap();
    drop(writer);

    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["history_health"] == "unavailable"
    });
    assert!(snapshot["codex"]["sqlite"].get("aggregate").is_none());
    assert!(snapshot["mcp"].get("history_aggregate").is_none());

    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute(
        "UPDATE history_state SET metadata=?1 WHERE namespace='activity'",
        [r#"{"version":3,"retention_days":90}"#],
    )
    .unwrap();
    let body = "{}";
    let response = service.request_body(
        &format!("POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()),
        body,
    );
    assert_eq!(response.status, 200);
    let recovered = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["history_health"] == "ok"
    });
    assert!(recovered["codex"]["sqlite"]["aggregate"].is_object());
    assert!(recovered["mcp"]["history_aggregate"].is_object());
    service.stop();
}

#[test]
fn core_sql_diagnostics_follow_the_wal_and_publish_only_metadata() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["errors"]["diagnostics"]["health"]["core"] == "missing"
    });
    // A directory notification may identify another newly created entry first.
    fs::create_dir(home.path().join("unrelated")).unwrap();
    let path = home.path().join("logs_active.sqlite");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.execute_batch(
        "CREATE TABLE logs(id INTEGER PRIMARY KEY, ts REAL, level TEXT, target TEXT, thread_id TEXT, feedback_log_body TEXT, message TEXT)",
    ).unwrap();
    let time = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .timestamp_millis() as f64
        / 1000.0;
    writer.execute("INSERT INTO logs VALUES(1,?,'INFO','sqlx::query',NULL,'',?)", rusqlite::params![
        time, r#"summary="SELECT PRIVATE_CORE" elapsed=241µs rows_affected=0 rows_returned=3 password="PRIVATE elapsed=900ms""#
    ]).unwrap();
    writer
        .execute(
            "INSERT INTO logs VALUES(2,?,'WARN','codex_core::responses_retry',NULL,?,NULL)",
            rusqlite::params![
                time,
                "stream disconnected request_id=sk-PRIVATE requestId=example-request trace_id=sk_PRIVATE PRIVATE_WARNING"
            ],
        )
        .unwrap();
    writer
        .execute(
            "INSERT INTO logs VALUES(3,?,'INFO','codex_core',NULL,?,NULL)",
            rusqlite::params![time, "PRIVATE_PROMPT"],
        )
        .unwrap();
    let initial = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["total"] == 1 && value["errors"]["total"] == 1
    });
    let sql = &initial["codex"]["sqlite"]["events"][0];
    assert_eq!(sql["source"], "codex_core");
    assert_eq!(sql["statement"], "SELECT");
    assert_eq!(sql["duration_ms"], 0.241);
    assert_eq!(sql["rows_affected"], 0);
    assert_eq!(sql["rows_returned"], 3);
    assert_eq!(sql["record_id"], 1);
    assert!(sql["container_duration_ms"].is_null());
    assert_eq!(initial["errors"]["events"][0]["code"], "stream_interrupted");
    assert_eq!(
        initial["errors"]["events"][0]["request_id"],
        "example-request"
    );
    let log = service.get("/api/logs").json();
    let source = log["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source"] == "codex_core")
        .unwrap();
    assert_eq!(source["files"][0]["record_id"], 3);
    assert_eq!(source["read_lines"], 3);
    assert!(!initial.to_string().contains("PRIVATE"));
    assert!(!log.to_string().contains("PRIVATE"));

    writer
        .execute(
            "INSERT INTO logs VALUES(4,?,'INFO','codex_state::state_db',NULL,?,NULL)",
            rusqlite::params![
                time,
                r#"sql="UPDATE fixture SET x='PRIVATE_APPEND'" elapsed_secs=0.0025 rows_affected=7"#
            ],
        )
        .unwrap();
    let appended = snapshot_when(&service, |value| value["codex"]["sqlite"]["total"] == 2);
    assert_eq!(appended["codex"]["sqlite"]["aggregate"]["total"], 2);
    let events = appended["codex"]["sqlite"]["events"].as_array().unwrap();
    let latest = events.iter().find(|row| row["record_id"] == 4).unwrap();
    assert_eq!(latest["duration_ms"], 2.5);
    assert_eq!(latest["rows_affected"], 7);
    assert_eq!(service.get("/api/history?section=sql").json()["total"], 2);
    assert_eq!(
        writer
            .query_row("SELECT count(*) FROM logs", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        4
    );
    service.stop();
}

#[test]
fn core_model_api_projects_requests_connections_and_retries() {
    let home = tempdir().unwrap();
    let writer = rusqlite::Connection::open(home.path().join("logs_4.sqlite")).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
        )
        .unwrap();
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).timestamp();
    for (id, module, body) in [
        (
            1,
            "codex_otel.log_only",
            r#"event.name="codex.api_request" endpoint="/responses" status_code=200 input_tokens=0 body="PRIVATE""#,
        ),
        (
            2,
            "codex_api::endpoint::responses_websocket",
            "successfully connected to websocket model=fixture",
        ),
        (
            3,
            "codex_core::responses_retry",
            r#"stream disconnected retries=1 sampling_error="PRIVATE""#,
        ),
        (
            4,
            "codex_otel.log_only",
            r#"message="event.name=codex.api_request""#,
        ),
    ] {
        writer
            .execute(
                "INSERT INTO logs VALUES(?,?,'INFO',?,?)",
                rusqlite::params![id, stamp, module, body],
            )
            .unwrap();
    }
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["model_api"]["events"]
            .as_array()
            .is_some_and(|events| events.len() == 3)
    });
    let api = &snapshot["model_api"];
    assert_eq!(
        (
            api["requests"].as_u64(),
            api["connections"].as_u64(),
            api["retry_events"].as_u64()
        ),
        (Some(1), Some(1), Some(1))
    );
    assert_eq!(
        api["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["record_id"] == 1)
            .unwrap()["input_tokens"],
        0
    );
    assert!(!api.to_string().contains("PRIVATE"));
    let recent = service.get("/api/snapshot?window=1h").json();
    assert_eq!(recent["model_api"]["requests"], 1);
    assert_eq!(recent["model_api"]["connections"], 1);
    assert_eq!(recent["model_api"]["retry_events"], 1);
    writer
        .execute(
            "INSERT INTO logs VALUES(5,?,'INFO','codex_otel.log_only',?)",
            rusqlite::params![
                stamp,
                r#"event.name="codex.api_request" status_code=429 request_id=sk-PRIVATE"#
            ],
        )
        .unwrap();
    let updated = snapshot_when(&service, |value| value["model_api"]["requests"] == 2);
    assert_eq!(updated["model_api"]["failed_requests"], 1);
    assert!(!updated["model_api"].to_string().contains("PRIVATE"));
    service.stop();
    let mut restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| value["model_api"]["requests"] == 2);
    assert_eq!(restored["model_api"]["events"].as_array().unwrap().len(), 4);
    assert_eq!(restored["model_api"]["failed_requests"], 1);
    restarted.stop();
}

#[test]
fn core_model_api_keeps_the_most_recent_thousand_events() {
    let home = tempdir().unwrap();
    let mut writer = rusqlite::Connection::open(home.path().join("logs_7.sqlite")).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
        )
        .unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).timestamp();
    {
        let tx = writer.transaction().unwrap();
        for id in 1..=1001 {
            tx.execute(
                "INSERT INTO logs VALUES(?,?,'INFO','codex_otel.log_only',?)",
                rusqlite::params![
                    id,
                    now - 1001 + id,
                    r#"event.name="codex.api_request" status_code=200"#
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["model_api"]["requests"] == 1000);
    let events = snapshot["model_api"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 1000);
    assert_eq!(events.first().unwrap()["record_id"], 1001);
    assert_eq!(events.last().unwrap()["record_id"], 2);
    service.stop();
}

#[test]
fn core_sql_diagnostics_retry_the_pending_page_after_history_write_failure() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let history =
        rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    history.execute_batch("CREATE TRIGGER reject_diagnostic BEFORE INSERT ON history_items
                            WHEN NEW.section='sql' BEGIN SELECT RAISE(ABORT,'fixture failure'); END").unwrap();

    let source = rusqlite::Connection::open(home.path().join("logs_1.sqlite")).unwrap();
    source.pragma_update(None, "journal_mode", "WAL").unwrap();
    source
        .execute_batch(
            "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
        )
        .unwrap();
    let time = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .timestamp_millis() as f64
        / 1000.0;
    let append = |id| {
        source
            .execute(
                "INSERT INTO logs VALUES(?,?,'INFO','sqlx::query',?)",
                rusqlite::params![id, time, r#"sql="SELECT PRIVATE_BODY" elapsed=2ms"#],
            )
            .unwrap();
    };
    append(1);
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["codex"]["sqlite"]["total"] == 1);
    let logs = service.get("/api/logs").json();
    let core = logs["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["source"] == "codex_core")
        .unwrap();
    assert_eq!(core["storage_health"], "unavailable");
    assert_eq!(service.get("/api/history?section=sql").json()["total"], 0);
    append(2);
    history
        .execute_batch("DROP TRIGGER reject_diagnostic")
        .unwrap();
    append(3);
    let recovered = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["aggregate"]["total"] == 3
    });
    assert_eq!(recovered["codex"]["sqlite"]["total"], 3);
    assert_eq!(service.get("/api/history?section=sql").json()["total"], 3);
    assert!(!recovered.to_string().contains("PRIVATE"));
    service.stop();
}

#[test]
fn error_history_restores_recent_metadata_after_sources_disappear() {
    let home = tempdir().unwrap();
    let (session, _) = native_event_fixture(home.path());
    let logs = home.path().join("logs_1.sqlite");
    let db = rusqlite::Connection::open(&logs).unwrap();
    db.execute_batch(
        "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
    )
    .unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let time = now.timestamp_millis() as f64 / 1000.0;
    db.execute(
        "INSERT INTO logs VALUES(1,?,'WARN','codex_core::responses_retry',?)",
        rusqlite::params![
            time,
            "stream disconnected request_id=fixture-1 PRIVATE_BODY"
        ],
    )
    .unwrap();
    drop(db);

    let history_path = "/api/history?namespace=errors&section=events";
    let mut service = Service::start(home.path(), true, None);
    let first = snapshot_when(&service, |value| value["errors"]["total"] == 3);
    assert_eq!(first["errors"]["history"]["retained"], 2);
    assert_eq!(first["errors"]["history"]["health"], "ok");
    let saved = service.get(history_path).json();
    assert_eq!(saved["total"], 3);
    assert!(!saved.to_string().contains("PRIVATE_"));
    let initial = |page: &Value| {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["code"] == "initial_failure")
            .unwrap()
            .clone()
    };
    let observed = initial(&saved);
    let raw = fs::read(&session).unwrap();
    let offset = observed["record_offset"].as_u64().unwrap() as usize;
    let line = raw[offset..].split(|byte| *byte == b'\n').next().unwrap();
    use sha2::{Digest, Sha256};
    assert_eq!(
        observed["record_hash"],
        format!("{:x}", Sha256::digest(line))
    );
    assert_eq!(
        observed["file"],
        session.file_name().unwrap().to_str().unwrap()
    );
    service.stop();

    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let payload: String = db
        .query_row("SELECT payload FROM session_cursors", [], |row| row.get(0))
        .unwrap();
    let mut v1: Value = serde_json::from_str(&payload).unwrap();
    v1["version"] = 1.into();
    for error in v1["projection"]["errors"].as_array_mut().unwrap() {
        let error = error.as_object_mut().unwrap();
        error.remove("record_offset");
        error.remove("record_hash");
    }
    db.execute("UPDATE session_cursors SET payload=?", [v1.to_string()])
        .unwrap();
    db.execute(
        "DELETE FROM history_items WHERE namespace='errors' AND json_extract(payload,'$.code')='old_failure'",
        [],
    ).unwrap();
    drop(db);
    let mut service = Service::start(home.path(), true, None);
    let from_v1 = snapshot_when(&service, |value| {
        value["errors"]["history"]["retained"] == 2
    });
    assert_eq!(
        from_v1["codex"]["read_state"]["checkpoint"]["invalidated"],
        1
    );
    assert!(from_v1["codex"]["bytes_read"].as_u64().unwrap() > 0);
    assert!(from_v1["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| {
            row["code"] == "initial_failure" && row["record_hash"] == observed["record_hash"]
        }));
    let rebuilt = service.get(history_path).json();
    assert_eq!(rebuilt["total"], 3);
    assert_eq!(initial(&rebuilt), observed);
    assert!(rebuilt["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| { row["code"] == "old_failure" && row["record_hash"].is_string() }));
    service.stop();

    fs::remove_file(session).unwrap();
    fs::remove_file(logs).unwrap();
    let mut service = Service::start(home.path(), true, None);
    let restored = snapshot_when(&service, |value| value["errors"]["total"] == 2);
    assert_eq!(restored["errors"]["history"]["retained"], 2);
    let logs = service.get("/api/logs?window=1h").json();
    assert_eq!(logs["total"], 2);
    assert!(logs["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["record_id"] == 1 && row["request_id"] == "fixture-1"));
    assert!(!logs.to_string().contains("PRIVATE_"));
    assert_eq!(service.get(history_path).json(), rebuilt);
    service.stop();

    let mut disabled = Service::start(home.path(), false, None);
    let snapshot = disabled.get("/api/snapshot?window=all").json();
    assert_eq!(snapshot["errors"]["history"]["health"], "disabled");
    assert_eq!(snapshot["errors"]["total"], 0);
    assert_eq!(disabled.get(history_path).json(), rebuilt);
    disabled.stop();
}

#[test]
fn existing_error_history_filters_credential_identifiers_on_read() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let payload = json!({"timestamp":stamp,"source":"codex_core","category":"codex",
        "severity":"error","code":"sk-PRIVATE","request_id":"sk-PRIVATE",
        "trace_id":"sk_PRIVATE","method":"safe"})
    .to_string();
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .save_history(
            Namespace::Errors,
            r#"{"version":1}"#,
            0,
            None,
            &[HistoryWrite {
                section: Section::Errors,
                key: "fixture",
                ordinal: 0,
                event_time: Some(&stamp),
                payload: &payload,
            }],
        )
        .unwrap();
    drop(store);
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["errors"]["total"] == 1);
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let history = service
        .get("/api/history?namespace=errors&section=events")
        .json();
    assert_eq!(history["items"][0]["method"], "safe");
    assert!(!history.to_string().contains("PRIVATE"));
    let source = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    let stored: String = source
        .query_row(
            "SELECT payload FROM history_items WHERE item_key='fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, payload);
    service.stop();
}

#[test]
fn existing_mcp_history_filters_credential_resources_on_read() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let payload = json!({"timestamp":stamp,"server":"fixture","tool":"read",
        "metadata":{"resources":{"page_id":"sk-PRIVATE","runId":"safe"}},
        "result":{"threadId":"sk_PRIVATE","projectId":"safe"}})
    .to_string();
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .save_history(
            Namespace::Activity,
            r#"{"version":3,"retention_days":90}"#,
            0,
            None,
            &[HistoryWrite {
                section: Section::Mcp,
                key: "fixture",
                ordinal: 0,
                event_time: Some(&stamp),
                payload: &payload,
            }],
        )
        .unwrap();
    drop(store);
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["mcp"]["total_events"] == 1);
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let history = service.get("/api/history?section=mcp").json();
    assert_eq!(
        history["items"][0]["metadata"]["resources"],
        json!({"runId":"safe"})
    );
    assert_eq!(history["items"][0]["result"], json!({"projectId":"safe"}));
    assert!(!history.to_string().contains("PRIVATE"));
    let source = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    let stored: String = source
        .query_row(
            "SELECT payload FROM history_items WHERE item_key='fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, payload);
    service.stop();
}

#[test]
fn error_history_write_failure_retries_checkpoint_and_diagnostics() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    let burst: Vec<_> = (0..55)
        .map(|index| {
            event(
                &stamp,
                "event_msg",
                json!({"type":"turn_aborted","error_code":format!("burst_{index}")}),
            )
        })
        .collect();
    append_records(&session, &burst);
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_errors BEFORE INSERT ON history_items
         WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture failure'); END",
    )
    .unwrap();
    let logs = rusqlite::Connection::open(home.path().join("logs_1.sqlite")).unwrap();
    logs.execute_batch(
        "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
    )
    .unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    logs.execute(
        "INSERT INTO logs VALUES(1,?,'WARN','sqlx::query',?)",
        rusqlite::params![now.timestamp(), r#"sql="SELECT PRIVATE_QUERY" elapsed=2ms"#],
    )
    .unwrap();
    let mut service = Service::start(home.path(), true, None);
    let first = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "unavailable"
    });
    assert_eq!(first["errors"]["total"], 51);
    assert_eq!(first["errors"]["history"]["health"], "unavailable");
    assert_eq!(first["errors"]["history"]["error_type"], "SQLiteError");
    let history_path = "/api/history?namespace=errors&section=events";
    assert_eq!(service.get(history_path).json()["total"], 0);
    assert_eq!(service.get("/api/history?section=sql").json()["total"], 0);
    let key = checkpoint_key(&session);
    let mut store = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
    assert!(store.checkpoint(&key, 10).unwrap().is_none());

    db.execute_batch("DROP TRIGGER reject_errors").unwrap();
    append_records(
        &session,
        &[event(
            &stamp,
            "event_msg",
            json!({"type":"turn_aborted","error_code":"user_cancelled"}),
        )],
    );
    let recovered = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "ok" && value["errors"]["total"] == 58
    });
    assert_eq!(recovered["errors"]["history"]["retained"], 58);
    assert_eq!(service.get(history_path).json()["total"], 59);
    assert_eq!(service.get("/api/history?section=sql").json()["total"], 1);
    let saved = store.checkpoint(&key, 10).unwrap().unwrap();
    assert_eq!(
        saved.cursor.offset,
        fs::metadata(&session).unwrap().len() as i64
    );
    service.stop();
}

#[test]
fn error_history_keeps_errors_beyond_the_session_preview() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    let errors: Vec<_> = (0..55)
        .map(|index| {
            event(
                &stamp,
                "event_msg",
                json!({"type":"turn_aborted","error_code":format!("fixture_{index}")}),
            )
        })
        .collect();
    append_records(&session, &errors);
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "ok"
    });
    assert_eq!(snapshot["errors"]["history"]["retained"], 56);
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        57
    );
    service.stop();
}

#[test]
fn session_error_code_excludes_credential_like_values() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    append_records(
        &session,
        &[event(
            &stamp,
            "event_msg",
            json!({
                "type":"error","error":{"code":"sk-PRIVATE"}
            }),
        )],
    );
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["errors"]["total"] == 3);
    assert!(!snapshot.to_string().contains("PRIVATE"));
    assert!(snapshot["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["code"] == "error"));
    let history = service
        .get("/api/history?namespace=errors&section=events")
        .json();
    assert!(!history.to_string().contains("PRIVATE"));
    service.stop();
}

#[test]
fn observed_tool_failure_reaches_snapshot_history_and_restart() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call",
            "name":"mcp__fixture__read","call_id":"fixture-call","arguments":"{}"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output",
            "call_id":"fixture-call","output":{"isError":true,"error":{
                "code":"fixture_failure","message":"timeout request_id=req-safe PRIVATE_BODY",
                "trace_id":"sk-PRIVATE","status_code":429}}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call",
            "name":"functions.exec_command","call_id":"process","arguments":"{}"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output",
            "call_id":"process","output":"Process exited with code 7\n"}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["errors"]["total"] == 4);
    let events = snapshot["errors"]["events"].as_array().unwrap();
    let mcp = events
        .iter()
        .find(|event| event["call_id"] == "fixture-call")
        .unwrap();
    assert_eq!(mcp["code"], "fixture_failure");
    assert_eq!(mcp["category"], "mcp");
    assert_eq!(mcp["server"], "fixture");
    assert_eq!(mcp["tool"], "read");
    assert_eq!(mcp["request_id"], "req-safe");
    assert_eq!(mcp["http_status"], 429);
    assert_eq!(mcp["cause"], "timeout");
    let process = events
        .iter()
        .find(|event| event["call_id"] == "process")
        .unwrap();
    assert_eq!(process["code"], "process_exit");
    assert_eq!(process["exit_code"], 7);
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let history = service
        .get("/api/history?namespace=errors&section=events")
        .json();
    assert_eq!(history["total"], 4);
    assert!(!history.to_string().contains("PRIVATE"));
    service.stop();

    let mut restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| value["errors"]["total"] == 4);
    assert_eq!(restored["codex"]["read_state"]["checkpoint"]["restored"], 1);
    assert_eq!(restored["codex"]["bytes_read"], 0);
    assert_eq!(
        restarted
            .get("/api/history?namespace=errors&section=events")
            .json(),
        history
    );
    restarted.stop();
}

#[test]
fn unsaved_tool_failure_uses_call_start_for_window_selection() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let start = (lam_history_store::parse_utc_stamp(&stamp).unwrap() - chrono::Duration::hours(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    append_records(
        &path,
        &[
            event(
                &start,
                "response_item",
                json!({"type":"function_call","name":"mcp__fixture__read","call_id":"old","arguments":"{}"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"old","output":{"isError":true}}),
            ),
        ],
    );
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_errors BEFORE INSERT ON history_items
        WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture failure'); END",
    )
    .unwrap();
    let mut service = Service::start(home.path(), true, None);
    let all = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "unavailable"
    });
    assert!(all["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "old"));
    let recent = service.get("/api/snapshot?window=1h").json();
    assert!(!recent["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "old"));
    service.stop();
}

#[test]
fn unsaved_session_error_without_thread_is_not_in_live_snapshot() {
    let home = tempdir().unwrap();
    let sessions = home.path().join("sessions");
    fs::create_dir(&sessions).unwrap();
    let path = sessions.join("rollout-without-id.jsonl");
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(
        &path,
        event(
            &stamp,
            "event_msg",
            json!({"type":"error","error":{"code":"fixture_failure"}}),
        ),
    )
    .unwrap();
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_errors BEFORE INSERT ON history_items
        WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture failure'); END",
    )
    .unwrap();
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "unavailable"
    });
    assert_eq!(snapshot["errors"]["total"], 0);
    db.execute_batch("DROP TRIGGER reject_errors").unwrap();
    append_records(&path, &[event(&stamp, "event_msg", json!({"type":"noop"}))]);
    let recovered = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "ok" && value["errors"]["history"]["retained"] == 1
    });
    assert_eq!(recovered["errors"]["total"], 1);
    assert!(recovered["errors"]["events"][0]["thread_id"].is_null());
    service.stop();
}

#[test]
fn nested_tool_failures_keep_outer_result_ownership() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    for (id, input) in [
        ("isolated", "await tools.mcp__fixture__read({});"),
        (
            "multiple",
            "await tools.mcp__fixture__read({}); await tools.exec_command({cmd:'x'});",
        ),
    ] {
        append_records(
            &path,
            &[
                event(
                    &stamp,
                    "response_item",
                    json!({"type":"custom_tool_call","name":"functions.exec","call_id":id,"input":input}),
                ),
                event(
                    &stamp,
                    "response_item",
                    json!({"type":"custom_tool_call_output","call_id":id,
                "output":{"isError":true,"error":{"code":"fixture_failure","message":"PRIVATE_BODY"}}}),
                ),
            ],
        );
    }
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["errors"]["total"] == 4);
    let events = snapshot["errors"]["events"].as_array().unwrap();
    let isolated = events
        .iter()
        .find(|event| event["call_id"] == "isolated")
        .unwrap();
    assert_eq!(isolated["category"], "mcp");
    assert_eq!(isolated["server"], "fixture");
    assert_eq!(isolated["tool"], "read");
    let multiple = events
        .iter()
        .find(|event| event["call_id"] == "multiple")
        .unwrap();
    assert_eq!(multiple["category"], "tool");
    assert!(multiple["server"].is_null());
    assert_eq!(multiple["tool"], "functions.exec");
    assert!(!snapshot.to_string().contains("PRIVATE"));
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        4
    );
    service.stop();
}

#[test]
fn older_call_checkpoint_replays_to_recover_tool_failure() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call",
            "name":"mcp__fixture__read","call_id":"fixture-call","arguments":"{}"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output",
            "call_id":"fixture-call","output":{"isError":true,"error":{"code":"fixture_failure"}}}),
            ),
        ],
    );
    let mut initial = Service::start(home.path(), true, None);
    snapshot_when(&initial, |value| value["errors"]["total"] == 3);
    initial.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    db.execute_batch(
        "UPDATE session_cursors SET payload=json_set(payload,'$.version',3);
         UPDATE session_calls SET payload=json_remove(payload,'$.failure');
         DELETE FROM history_items WHERE namespace='errors' AND json_extract(payload,'$.source')='tool_result';",
    ).unwrap();
    drop(db);
    let mut restarted = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&restarted, |value| value["errors"]["total"] == 3);
    assert_eq!(
        snapshot["codex"]["read_state"]["checkpoint"]["invalidated"],
        1
    );
    assert!(snapshot["codex"]["bytes_read"].as_u64().unwrap() > 0);
    assert!(snapshot["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["code"] == "fixture_failure"));
    assert_eq!(
        restarted
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        3
    );
    restarted.stop();
}

#[test]
fn tool_failure_and_checkpoint_roll_back_together() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let old_size = fs::metadata(&path).unwrap().len() as i64;
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_tool_error BEFORE INSERT ON history_items
        WHEN NEW.namespace='errors' AND json_extract(NEW.payload,'$.source')='tool_result'
        BEGIN SELECT RAISE(ABORT,'fixture failure'); END",
    )
    .unwrap();
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"mcp__fixture__read","call_id":"failed","arguments":"{}"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"failed","output":{"isError":true}}),
            ),
        ],
    );
    let failed = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "unavailable"
    });
    assert!(failed["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "failed"));
    let mut store = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
    assert_eq!(
        store
            .checkpoint(&checkpoint_key(&path), 10)
            .unwrap()
            .unwrap()
            .cursor
            .offset,
        old_size
    );
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        2
    );
    db.execute_batch("DROP TRIGGER reject_tool_error").unwrap();
    let new_size = fs::metadata(&path).unwrap().len() as i64;
    fs::remove_file(&path).unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
            && value["errors"]["history"]["retained"] == 2
    });
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        3
    );
    assert_eq!(
        store
            .checkpoint(&checkpoint_key(&path), 10)
            .unwrap()
            .unwrap()
            .cursor
            .offset,
        new_size
    );
    service.stop();
}

#[test]
fn repeated_tool_output_without_payload_clears_previous_failure() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"mcp__fixture__read","call_id":"repeat","arguments":"{}"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"repeat","output":{"isError":true}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"repeat"}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 1);
    assert_eq!(snapshot["errors"]["total"], 2);
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        2
    );
    service.stop();
}

#[test]
fn oversized_tool_responses_are_skipped_by_session_framing() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let oversized = json!({"status":"failed","padding":"x".repeat(1024 * 1024)}).to_string();
    let late = format!("{}\nProcess exited with code 7\n", "x".repeat(1024 * 1024));
    let unicode = json!({"status":"failed","padding":"漢".repeat(512 * 1024)}).to_string();
    for (id, output) in [
        ("oversized", oversized),
        ("late", late),
        ("unicode", unicode),
    ] {
        append_records(
            &path,
            &[
                event(
                    &stamp,
                    "response_item",
                    json!({"type":"function_call","name":"functions.exec_command","call_id":id,"arguments":"{}"}),
                ),
                event(
                    &stamp,
                    "response_item",
                    json!({"type":"function_call_output","call_id":id,"output":output}),
                ),
            ],
        );
    }
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 3);
    let events = snapshot["errors"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    assert!(!events.iter().any(|event| event["call_id"] == "oversized"
        || event["call_id"] == "late"
        || event["call_id"] == "unicode"));
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        2
    );
    service.stop();
}

#[test]
fn unmatched_tool_failures_reach_snapshot_and_history() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"orphan",
            "output":{"isError":true,"error":{"code":"fixture_failure","message":"timeout PRIVATE_BODY"}}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call_output",
            "output":"Process exited with code 7\n"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"ok",
            "output":{"status":"ok"}}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["errors"]["total"] == 4);
    let events = snapshot["errors"]["events"].as_array().unwrap();
    let orphan = events
        .iter()
        .find(|event| event["call_id"] == "orphan")
        .unwrap();
    assert_eq!(orphan["source"], "tool_result");
    assert_eq!(orphan["category"], "tool");
    assert_eq!(orphan["code"], "fixture_failure");
    assert_eq!(orphan["cause"], "timeout");
    assert!(orphan["record_hash"].is_string());
    let process = events
        .iter()
        .find(|event| event["code"] == "process_exit")
        .unwrap();
    assert!(process["call_id"].is_null());
    assert_eq!(process["exit_code"], 7);
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let history = service
        .get("/api/history?namespace=errors&section=events")
        .json();
    assert_eq!(history["total"], 4);
    assert!(!history.to_string().contains("PRIVATE"));
    service.stop();

    let mut restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| value["errors"]["total"] == 4);
    assert_eq!(restored["codex"]["read_state"]["checkpoint"]["restored"], 1);
    assert_eq!(restored["codex"]["bytes_read"], 0);
    assert_eq!(
        restarted
            .get("/api/history?namespace=errors&section=events")
            .json(),
        history
    );
    restarted.stop();

    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let payload: String = db
        .query_row("SELECT payload FROM session_cursors", [], |row| row.get(0))
        .unwrap();
    let mut previous: Value = serde_json::from_str(&payload).unwrap();
    previous["version"] = 3.into();
    previous["projection"]["errors"]
        .as_array_mut()
        .unwrap()
        .retain(|error| error.get("failure").is_none());
    db.execute(
        "UPDATE session_cursors SET payload=?",
        [previous.to_string()],
    )
    .unwrap();
    db.execute("DELETE FROM history_items WHERE namespace='errors' AND json_extract(payload,'$.source')='tool_result'", []).unwrap();
    drop(db);
    let mut upgraded = Service::start(home.path(), true, None);
    let replayed = snapshot_when(&upgraded, |value| value["errors"]["total"] == 4);
    assert_eq!(
        replayed["codex"]["read_state"]["checkpoint"]["invalidated"],
        1
    );
    assert!(replayed["codex"]["bytes_read"].as_u64().unwrap() > 0);
    assert_eq!(
        upgraded
            .get("/api/history?namespace=errors&section=events")
            .json(),
        history
    );
    upgraded.stop();
}

#[test]
fn unmatched_tool_failure_retries_after_write_failure_and_source_removal() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output",
        "call_id":"orphan","output":{"isError":true,"error":{"code":"fixture_failure"}}}),
        )],
    );
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_orphan BEFORE INSERT ON history_items
        WHEN NEW.namespace='errors' AND json_extract(NEW.payload,'$.source')='tool_result'
        BEGIN SELECT RAISE(ABORT,'fixture failure'); END",
    )
    .unwrap();
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "unavailable"
    });
    assert!(snapshot["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "orphan"));
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        0
    );
    db.execute_batch("DROP TRIGGER reject_orphan").unwrap();
    fs::remove_file(path).unwrap();
    let recovered = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "ok" && value["errors"]["history"]["retained"] == 2
    });
    assert!(recovered["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "orphan"));
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        3
    );
    service.stop();
}

#[test]
fn corrupt_error_locator_invalidates_checkpoint_before_publication() {
    let home = tempdir().unwrap();
    let (_session, _) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    service.stop();

    let path = home
        .path()
        .join("monitoring")
        .join(lam_history_store::FILE_NAME);
    let db = rusqlite::Connection::open(path).unwrap();
    let payload: String = db
        .query_row("SELECT payload FROM session_cursors", [], |row| row.get(0))
        .unwrap();
    let mut payload: Value = serde_json::from_str(&payload).unwrap();
    payload["projection"]["errors"][0]["record_hash"] = "PRIVATE_BAD_HASH".into();
    db.execute(
        "UPDATE session_cursors SET payload=?",
        [payload.to_string()],
    )
    .unwrap();
    drop(db);

    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["invalidated"] == 1
    });
    assert!(snapshot["codex"]["bytes_read"].as_u64().unwrap() > 0);
    assert!(!snapshot.to_string().contains("PRIVATE_BAD_HASH"));
    service.stop();
}

#[test]
fn retention_setting_is_persisted_without_enabling_collection() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        90
    );
    assert!(!home.path().join("monitoring").exists());

    let post = |body: &str| {
        service.request_body(
            &format!(
                "POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
                service.port,
                body.len(),
            ),
            body,
        )
    };
    let body = r#"{"activity_retention_days":1}"#;
    let request = |headers: &str| {
        service.request_body(
        &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: {}\r\n{headers}", service.port, body.len()),
        body,
    )
    };
    assert_eq!(request("Content-Type: text/plain\r\n").status, 400);
    assert_eq!(
        request("Content-Type: application/json\r\nOrigin: http://external.invalid\r\n").status,
        403
    );
    assert!(!home.path().join("monitoring").exists());
    for body in [
        r#"{"activity_retention_days":-1}"#,
        r#"{"activity_retention_days":3651}"#,
        r#"{"activity_retention_days":1.5}"#,
        r#"{"activity_retention_days":true}"#,
        r#"{"activity_retention_days":null}"#,
        r#"{"activity_retention_days":"1"}"#,
    ] {
        assert_eq!(post(body).status, 409, "{body}");
    }
    assert!(!home.path().join("monitoring").exists());
    assert_eq!(post(r#"{"activity_retention_days":3650}"#).status, 200);
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        3650
    );
    assert_eq!(post(r#"{"activity_retention_days":-1}"#).status, 409);
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        3650
    );
    assert_eq!(
        post(r#"{"activity_retention_days":1,"debug_mode":true}"#).status,
        409
    );
    let mut stream = EventStream::connect(&service, "?window=24h").unwrap();
    let initial_version = stream.snapshot()["version"].as_u64().unwrap();
    let saved = post(r#"{"activity_retention_days":0}"#);
    assert!(stream.snapshot()["version"].as_u64().unwrap() > initial_version);
    drop(stream);
    assert_eq!(saved.status, 200);
    assert_eq!(saved.json()["activity_retention_days"], 0);
    service.stop();

    let mut restarted = Service::start(home.path(), false, None);
    assert_eq!(
        restarted.get("/api/settings").json()["activity_retention_days"],
        0
    );
    assert_eq!(
        restarted.get("/api/snapshot").json()["codex"]["health"],
        "disabled"
    );
    restarted.stop();
}

#[test]
fn startup_resumes_incomplete_retention_cleanup() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch(
        "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM ids WHERE n<501)
         INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload)
         SELECT 'activity','web',n,0,'2000-01-01T00:00:00.000Z','{}' FROM ids;",
    )
    .unwrap();
    let now = chrono::DateTime::from(std::time::SystemTime::now());
    assert_eq!(store.set_activity_retention(1, now).unwrap(), 500);
    drop(store);
    let mut service = Service::start(home.path(), false, None);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining: i64 = db
            .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
            .unwrap();
        if remaining == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "cleanup did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
    service.stop();
}

#[test]
fn retention_cleanup_failure_can_be_retried_through_http() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch(
        "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM ids WHERE n<501)
         INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload)
         SELECT 'activity','web',n,0,'2000-01-01T00:00:00.000Z','{}' FROM ids;
         CREATE TRIGGER reject_last BEFORE DELETE ON history_items
         WHEN OLD.item_key='501'
         BEGIN SELECT RAISE(ABORT,'fixture failure'); END;",
    )
    .unwrap();
    let post = || {
        service.request_body(
        &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: 29\r\n", service.port),
        r#"{"activity_retention_days":1}"#,
    )
    };
    let mut stream = EventStream::connect(&service, "?window=all").unwrap();
    let initial_version = stream.snapshot()["version"].as_u64().unwrap();
    assert_eq!(post().status, 503);
    assert!(stream.snapshot()["version"].as_u64().unwrap() > initial_version);
    drop(stream);
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        1
    );
    let remaining: i64 = db
        .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
    db.execute_batch("DROP TRIGGER reject_last").unwrap();
    assert_eq!(post().status, 200);
    let remaining: i64 = db
        .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 0);
    service.stop();
}

#[test]
fn concurrent_retention_requests_leave_a_valid_setting() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    let port = service.port;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for days in [0, 90] {
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            let body = format!(r#"{{"activity_retention_days":{days}}}"#);
            barrier.wait();
            write!(stream, "POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            (days, response)
        }));
    }
    barrier.wait();
    let responses: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    for (days, response) in responses {
        if response.starts_with("HTTP/1.1 503 ") {
            let body = format!(r#"{{"activity_retention_days":{days}}}"#);
            let retry = service.request_body(
                &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", body.len()),
                &body,
            );
            assert_eq!(retry.status, 200);
        } else {
            assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        }
    }
    let days = service.get("/api/settings").json()["activity_retention_days"]
        .as_i64()
        .unwrap();
    assert!([0, 90].contains(&days));
    service.stop();
    let mut restarted = Service::start(home.path(), false, None);
    assert_eq!(
        restarted.get("/api/settings").json()["activity_retention_days"],
        days
    );
    restarted.stop();
}

#[test]
fn retention_update_removes_expired_history_from_disabled_snapshot() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp =
        (now - chrono::Duration::days(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let payload = json!({"timestamp":stamp,"server":"fixture","tool":"read"}).to_string();
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .save_history(
            Namespace::Activity,
            r#"{"version":3,"retention_days":90}"#,
            0,
            None,
            &[HistoryWrite {
                section: Section::Mcp,
                key: "fixture",
                ordinal: 0,
                event_time: Some(&stamp),
                payload: &payload,
            }],
        )
        .unwrap();
    drop(store);

    let mut service = Service::start(home.path(), false, None);
    let mut stream = EventStream::connect(&service, "?window=all").unwrap();
    assert_eq!(stream.snapshot()["snapshot"]["mcp"]["total_events"], 1);
    let body = r#"{"activity_retention_days":1}"#;
    let response = service.request_body(
        &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()),
        body,
    );
    assert_eq!(response.status, 200);
    let updated = stream.snapshot();
    assert_eq!(updated["snapshot"]["mcp"]["total_events"], 0);
    assert_eq!(updated["snapshot"]["codex"]["health"], "disabled");
    assert_eq!(service.get("/api/history?section=mcp").json()["total"], 0);
    drop(stream);
    service.stop();
}

#[test]
fn retention_update_removes_expired_history_from_enabled_snapshot() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp =
        (now - chrono::Duration::days(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let payload = json!({"timestamp":stamp,"server":"fixture","tool":"read"}).to_string();
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .save_history(
            Namespace::Activity,
            r#"{"version":3,"retention_days":90}"#,
            0,
            None,
            &[HistoryWrite {
                section: Section::Mcp,
                key: "fixture",
                ordinal: 0,
                event_time: Some(&stamp),
                payload: &payload,
            }],
        )
        .unwrap();
    drop(store);

    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["mcp"]["total_events"] == 1);
    let mut stream = EventStream::connect(&service, "?window=all").unwrap();
    assert_eq!(stream.snapshot()["snapshot"]["mcp"]["total_events"], 1);
    let body = r#"{"activity_retention_days":1}"#;
    let response = service.request_body(
        &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()),
        body,
    );
    assert_eq!(response.status, 200);
    let updated = stream.snapshot();
    assert_eq!(updated["snapshot"]["mcp"]["total_events"], 0);
    assert_ne!(updated["snapshot"]["codex"]["health"], "disabled");
    assert_eq!(service.get("/api/history?section=mcp").json()["total"], 0);
    drop(stream);
    service.stop();
}

#[test]
fn incomplete_settings_body_times_out_without_writing() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    let mut slow = TcpStream::connect((Ipv4Addr::LOCALHOST, service.port)).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
    write!(
        slow,
        "POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: 29\r\nConnection: close\r\n\r\n{{",
        service.port
    ).unwrap();
    assert_eq!(service.get("/api/instance").status, 200);
    let mut response = String::new();
    slow.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 400 "), "{response}");
    assert!(!home.path().join("monitoring").exists());
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        90
    );
    service.stop();
}

#[test]
fn invalid_retention_metadata_is_preserved_by_settings_api() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    let invalid = r#"{"version":3,"retention_days":"invalid"}"#;
    db.execute(
        "INSERT INTO history_state VALUES ('activity',?1,0)",
        [invalid],
    )
    .unwrap();
    let mut service = Service::start(home.path(), false, None);
    assert_eq!(service.get("/api/settings").status, 503);
    let body = r#"{"activity_retention_days":1}"#;
    let response = service.request_body(
        &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()),
        body,
    );
    assert_eq!(response.status, 503);
    let stored: String = db
        .query_row(
            "SELECT metadata FROM history_state WHERE namespace='activity'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, invalid);
    service.stop();
}

#[cfg(unix)]
#[test]
fn read_only_history_rejects_retention_update_without_changing_data() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    let now = chrono::DateTime::from(std::time::SystemTime::now());
    store.set_activity_retention(30, now).unwrap();
    drop(store);
    let database = monitoring.join(lam_history_store::FILE_NAME);
    let mut service = Service::start(home.path(), false, None);
    fs::set_permissions(&database, fs::Permissions::from_mode(0o400)).unwrap();
    fs::set_permissions(&monitoring, fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        30
    );
    let body = r#"{"activity_retention_days":1}"#;
    let response = service.request_body(
        &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()),
        body,
    );
    assert_eq!(response.status, 503);
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        30
    );
    service.stop();
    fs::set_permissions(&monitoring, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&database, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn source_update_recovers_unreadable_aggregate() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut writer = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    writer
        .save_history(Namespace::Activity, "invalid metadata", 1, None, &[])
        .unwrap();
    drop(writer);

    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["history_health"] == "unavailable"
    });
    assert!(snapshot["codex"]["sqlite"].get("aggregate").is_none());
    assert!(snapshot["mcp"].get("history_aggregate").is_none());

    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute(
        "UPDATE history_state SET metadata=?1 WHERE namespace='activity'",
        [r#"{"version":3,"retention_days":90}"#],
    )
    .unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"fixture","call_id":"recover"}),
        )],
    );
    let recovered = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["history_health"] == "ok"
    });
    assert!(recovered["codex"]["sqlite"]["aggregate"].is_object());
    assert!(recovered["mcp"]["history_aggregate"].is_object());
    assert_eq!(recovered["codex"]["observed_tool_calls"], 1);
    service.stop();
}

#[test]
fn session_created_immediately_after_service_start_is_collected() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), true, None);
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"fixture","call_id":"startup"}),
        )],
    );
    let snapshot = snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 1);
    assert_eq!(snapshot["codex"]["tools"]["fixture"], 1);
    service.stop();
}

#[test]
fn locked_retention_database_returns_error_then_accepts_retry() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .set_activity_retention(30, chrono::DateTime::from(std::time::SystemTime::now()))
        .unwrap();
    drop(store);
    let mut service = Service::start(home.path(), false, None);
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let body = r#"{"activity_retention_days":1}"#;
    let post = || {
        service.request_body(
        &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()),
        body,
    )
    };
    assert_eq!(post().status, 503);
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        30
    );
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(post().status, 200);
    assert_eq!(
        service.get("/api/settings").json()["activity_retention_days"],
        1
    );
    service.stop();
}

#[test]
fn startup_cleanup_retries_after_database_lock_is_released() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    let now = chrono::DateTime::from(std::time::SystemTime::now());
    store.set_activity_retention(1, now).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute("INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload) VALUES ('activity','web','old',0,'2000-01-01T00:00:00.000Z','{}')", []).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut service = Service::start(home.path(), false, None);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        db.query_row("SELECT count(*) FROM history_items", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    db.execute_batch("ROLLBACK").unwrap();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let remaining: i64 = db
            .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
            .unwrap();
        if remaining == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "cleanup did not retry");
        std::thread::sleep(Duration::from_millis(20));
    }
    service.stop();
}

#[test]
fn locked_startup_cleanup_does_not_prevent_service_shutdown() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .set_activity_retention(1, chrono::DateTime::from(std::time::SystemTime::now()))
        .unwrap();
    drop(store);
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute("INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload) VALUES ('activity','web','old',0,'2000-01-01T00:00:00.000Z','{}')", []).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut service = Service::start(home.path(), false, None);
    std::thread::sleep(Duration::from_millis(500));
    let started = Instant::now();
    service.stop();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(
        db.query_row("SELECT count(*) FROM history_items", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    db.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn desktop_fixture_is_opt_in_and_publishes_diagnostics() {
    let home = tempdir().unwrap();
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let path = directory.join("codex-desktop-fixture.log");
    fs::write(
        &path,
        format!("{now} ERROR [codex] stream disconnected request_id=safe secret=PRIVATE\n"),
    )
    .unwrap();
    let mut disabled = Service::start(home.path(), false, None);
    assert!(!disabled
        .get("/api/snapshot?window=all")
        .json()
        .to_string()
        .contains("safe"));
    disabled.stop();
    let mut enabled = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&enabled, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["source"] == "codex_desktop")
        })
    });
    assert!(!snapshot.to_string().contains("PRIVATE"));
    assert_eq!(
        enabled
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        1
    );
    enabled.stop();
}

#[test]
fn desktop_directory_created_after_start_publishes_appended_events() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["errors"]["diagnostics"]["health"]["desktop"] == "missing"
    });
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("codex-desktop-fixture.log");
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(&path, format!("{stamp} WARN [codex] first\n")).unwrap();
    snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["source"] == "codex_desktop")
        })
    });
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(format!("{stamp} ERROR [codex] second\n").as_bytes())
        .unwrap();
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .filter(|event| event["source"] == "codex_desktop")
                .count()
                == 2
        })
    });
    assert_eq!(snapshot["errors"]["diagnostics"]["health"]["desktop"], "ok");
    service.stop();
}

#[test]
fn desktop_diagnostic_retries_failed_storage_on_source_update() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_desktop BEFORE INSERT ON history_items WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["errors"]["diagnostics"]["health"]["desktop"] == "missing"
    });
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("codex-desktop-fixture.log");
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(
        &path,
        format!("{stamp} ERROR [codex] failed secret=PRIVATE\n"),
    )
    .unwrap();
    snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "unavailable"
    });
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        0
    );
    db.execute_batch("DROP TRIGGER reject_desktop").unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(format!("{stamp} INFO [codex] retry\n").as_bytes())
        .unwrap();
    let recovered = snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["source"] == "codex_desktop")
        })
    });
    assert!(!recovered.to_string().contains("PRIVATE"));
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        1
    );
    service.stop();
}
