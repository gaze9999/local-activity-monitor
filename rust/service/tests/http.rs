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
        Self::start_with_path(home, codex, assets, None)
    }

    fn start_with_path(
        home: &Path,
        codex: bool,
        assets: Option<&Path>,
        path: Option<&Path>,
    ) -> Self {
        let mut command = Command::new(BINARY);
        command
            .args(["--port", "0", "--watch-stdin", "--codex-home"])
            .arg(home);
        if let Some(path) = path {
            command.env("PATH", path);
        }
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

#[cfg(unix)]
#[test]
fn termination_signals_stop_service_cleanly() {
    for signal in ["TERM", "HUP"] {
        let home = tempdir().unwrap();
        let mut service = Service::start(home.path(), false, None);
        assert_eq!(service.get("/api/snapshot?window=all").status, 200);
        assert!(Command::new("kill")
            .args([format!("-{signal}"), service.child.id().to_string()])
            .status()
            .unwrap()
            .success());
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = service.child.try_wait().unwrap() {
                assert!(status.success(), "{signal}: {status}");
                break;
            }
            assert!(Instant::now() < deadline, "{signal} did not stop service");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
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
fn history_query_accepts_python_decimal_integer_syntax() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    for query in ["limit=%EF%BC%91_%D9%A2", "limit=%2B12", "limit=000_12"] {
        let response = service.get(&format!("/api/history?section=web&{query}&offset=-0"));
        assert_eq!(response.status, 200, "{query}");
        assert_eq!(response.json()["limit"], 12);
    }
    for query in ["limit=_12", "limit=12_", "limit=1__2", "limit=-1", "limit=201"] {
        assert_eq!(service.get(&format!("/api/history?section=web&{query}")).status, 400, "{query}");
    }
}

#[test]
fn snapshot_window_uses_first_nonempty_value_like_python() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    for query in ["window=", "unrelated=", "window=&window=24h", "window=24h&window=invalid"] {
        let response = service.get(&format!("/api/snapshot?{query}"));
        assert_eq!(response.status, 200, "{query}");
        assert_eq!(response.json()["monitor"]["window"], "24h");
    }
    for query in ["window=invalid&window=24h", "unrelated=1", "window=&window=invalid"] {
        assert_eq!(service.get(&format!("/api/snapshot?{query}")).status, 400, "{query}");
    }
    assert_eq!(service.get("/api/logs?window=").status, 400);
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
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn monitor_publishes_completed_gpu_collection() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let snapshot = snapshot_when(&service, |value| value["monitor"]["device_ready"] == true);
    assert!(snapshot["monitor"]["device_checked_at"].is_string());
    assert!(snapshot["monitor"]["gpus"]
        .as_array()
        .is_some_and(|items| items.len() <= 16));
}

#[test]
fn monitor_snapshot_publishes_bounded_font_families() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let snapshot = snapshot_when(&service, |value| value["monitor"]["fonts"].is_array());
    let fonts = snapshot["monitor"]["fonts"].as_array().unwrap();
    assert!(fonts.len() <= 500);
    assert!(fonts.iter().all(|value| value
        .as_str()
        .is_some_and(|name| !name.contains('/') && name.chars().count() <= 160)));
    assert!(fonts
        .windows(2)
        .all(|pair| pair[0].as_str() < pair[1].as_str()));
}

#[test]
fn service_stops_after_font_enumeration_starts() {
    let home = tempdir().unwrap();
    let mut service = Service::start(home.path(), false, None);
    service.stop();
}

#[test]
fn monitor_snapshot_exposes_process_runtime_information() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let snapshot = service.get("/api/snapshot").json();
    let monitor = &snapshot["monitor"];
    assert_eq!(monitor["process_usage"]["pid"], monitor["pid"]);
    assert_eq!(monitor["process_usage"]["storage_health"], "ok");
    assert!(monitor["process_usage"]["checked_at"].is_string());
    assert!(monitor["platform"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(monitor["architecture"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(monitor["process_bits"], std::mem::size_of::<usize>() * 8);
    assert!(monitor["logical_cpus"]
        .as_u64()
        .is_none_or(|value| value > 0));
    assert!(["system", "processor_group"].contains(&monitor["cpu_scope"].as_str().unwrap()));
    assert!(monitor["cpu_usage_percent"]
        .as_f64()
        .is_none_or(|value| (0.0..=100.0).contains(&value)));
    #[cfg(unix)]
    assert!(monitor["system_release"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(
        monitor["processor"].is_null()
            || monitor["processor"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
    );
    assert!(monitor["physical_memory_bytes"]
        .as_u64()
        .is_none_or(|value| value > 0));
    #[cfg(target_os = "macos")]
    {
        assert!(monitor["processor"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert!(monitor["physical_memory_bytes"]
            .as_u64()
            .is_some_and(|value| value > 0));
        assert!(monitor["available_memory_bytes"].is_null());
    }
}

#[test]
fn stream_does_not_increment_http_request_counter() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let first = service.get("/api/snapshot").json();
    let mut stream = EventStream::connect(&service, "").unwrap();
    let streamed = stream.snapshot();
    assert_eq!(
        streamed["snapshot"]["monitor"]["requests"],
        first["monitor"]["requests"].as_u64().unwrap() + 1
    );
    let response = service.get("/api/snapshot").json();
    assert_eq!(
        response["monitor"]["requests"],
        streamed["snapshot"]["monitor"]["requests"]
    );
}

#[test]
fn monitor_snapshot_reports_bounded_live_events_even_when_logs_disabled() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let initial = snapshot_when(&service, |value| {
        value["monitor"]["events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["kind"] == "started"))
    });
    assert_eq!(initial["monitor"]["event_limit"], 200);
    let body = json!({"observations":{"logs":false}}).to_string();
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body).status,200);
    assert_eq!(service.get("/private-fixture-missing").status, 404);
    let snapshot = snapshot_when(&service, |value| {
        value["monitor"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["kind"] == "http_response_error" && event["http_status"] == 404)
        })
    });
    assert_eq!(snapshot["settings"]["observations"]["logs"], false);
    assert!(!snapshot["monitor"]["events"]
        .to_string()
        .contains("private-fixture"));
}

#[test]
fn monitor_reports_http_requests_errors_and_snapshot_sizes() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let initial = service.get("/api/snapshot").json();
    assert_eq!(initial["monitor"]["requests"], 0);
    assert_eq!(initial["monitor"]["http_errors"], 0);
    assert_eq!(service.get("/missing-one").status, 404);
    assert_eq!(service.get("/missing-two").status, 404);
    let compressed = service.request(&format!(
        "GET /api/snapshot HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept-Encoding: gzip\r\n",
        service.port
    ));
    assert_eq!(compressed.header("content-encoding"), Some("gzip"));
    let mut decoded = Vec::new();
    GzDecoder::new(compressed.body.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    let after = service.get("/api/snapshot").json();
    assert_eq!(after["monitor"]["requests"], 4);
    assert_eq!(after["monitor"]["http_errors"], 2);
    assert_eq!(after["monitor"]["snapshot_bytes"], decoded.len());
    assert_eq!(after["monitor"]["transfer_bytes"], compressed.body.len());
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
fn sse_write_metrics_handle_pipelined_http_before_stream() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let body = r#"{"debug_mode":true}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    let mut socket =
        TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, service.port).into(), TIMEOUT).unwrap();
    socket.set_read_timeout(Some(TIMEOUT)).unwrap();
    write!(socket,"GET /api/snapshot HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\nGET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",service.port,service.port).unwrap();
    let mut reader = BufReader::new(socket);
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    assert!(status.contains("200"));
    let mut length = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let mut first = vec![0u8; length.expect("HTTP response length")];
    reader.read_exact(&mut first).unwrap();
    let mut status = String::new();
    reader.read_line(&mut status).unwrap();
    assert!(status.contains("200"));
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
    }
    let mut stream = EventStream(reader);
    let frame = stream.frame();
    let debug = stream.frame();
    let payload = debug.strip_prefix("event: debug\ndata: ").unwrap().trim();
    let debug: Value = serde_json::from_str(payload).unwrap();
    let record = debug["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "stream_frame")
        .unwrap();
    assert_eq!(record["transfer_bytes"], frame.len());
}

#[test]
fn debug_stream_reports_connection_and_snapshot() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let body = r#"{"debug_mode":true}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    let mut stream = EventStream::connect(&service, "").unwrap();
    let snapshot_frame = stream.frame();
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
    let body = snapshot_frame
        .split_once("\ndata: ")
        .unwrap()
        .1
        .strip_suffix("\n\n")
        .unwrap();
    assert_eq!(event["snapshot_bytes"], body.len());
    assert_eq!(event["transfer_bytes"], snapshot_frame.len());
    assert!(event["write_ms"].as_f64().is_some_and(|value| value >= 0.0));
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    assert!(event["cpu_ms"].as_f64().is_some_and(|value| value >= 0.0));
    let monitor = service.get("/api/snapshot").json();
    assert_eq!(monitor["monitor"]["snapshot_bytes"], body.len());
    assert_eq!(monitor["monitor"]["transfer_bytes"], snapshot_frame.len());
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
#[cfg(unix)]
fn debug_records_abrupt_stream_failure_without_request_data() {
    use std::os::fd::AsRawFd;
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let body = r#"{"debug_mode":true}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    let mut stream = EventStream::connect(&service, "").unwrap();
    stream.snapshot();
    let linger = libc::linger {
        l_onoff: 1,
        l_linger: 0,
    };
    let status = unsafe {
        libc::setsockopt(
            stream.0.get_ref().as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            (&linger as *const libc::linger).cast(),
            std::mem::size_of::<libc::linger>() as libc::socklen_t,
        )
    };
    assert_eq!(status, 0);
    drop(stream);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let refresh = "{}";
        service.request_body(&format!("POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,refresh.len()),refresh);
        let snapshot = service.get("/api/snapshot").json();
        if snapshot["monitor"]["debug"]["records"]
            .as_array()
            .is_some_and(|records| {
                records
                    .iter()
                    .any(|record| record["kind"] == "stream_error")
            })
        {
            break;
        }
        assert!(Instant::now() < deadline, "stream error was not recorded");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn plugin_metadata_appears_in_snapshot_and_obeys_metadata_switch() {
    let home = tempdir().unwrap();
    fs::write(
        home.path().join("config.toml"),
        "[plugins.\"fixture@provider\"]\nenabled = true\n",
    )
    .unwrap();
    let path = home.path().join("plugins/cache/provider/fixture/1.0.0");
    fs::create_dir_all(path.join("skills/example")).unwrap();
    fs::write(path.join("skills/example/SKILL.md"), "PRIVATE_SKILL_BODY").unwrap();
    fs::write(path.join("plugin.json"),r#"{"version":"1.0.0","skills":"./skills","mcpServers":{"demo":{"command":"PRIVATE_COMMAND"}}}"#).unwrap();
    let service = Service::start(home.path(), true, None);
    let found = snapshot_when(&service, |value| {
        value["codex"]["plugins"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == "fixture@provider"))
    });
    let item = &found["codex"]["plugins"]["items"][0];
    assert_eq!(item["enabled"], true);
    assert_eq!(item["skills_count"], 1);
    assert_eq!(item["mcp_servers"], json!(["demo"]));
    assert!(!found.to_string().contains("PRIVATE_COMMAND"));
    assert!(!found.to_string().contains("PRIVATE_SKILL_BODY"));
    fs::write(
        path.join("plugin.json"),
        r#"{"version":"1.0.0","skills":"./skills","mcpServers":{"demo":{},"second":{}}}"#,
    )
    .unwrap();
    let changed = snapshot_when(&service, |value| {
        value["codex"]["plugins"]["items"]
            .as_array()
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item["id"] == "fixture@provider" && item["mcp_count"] == 2)
            })
    });
    assert_eq!(
        changed["codex"]["plugins"]["items"][0]["mcp_servers"],
        json!(["demo", "second"])
    );
    fs::create_dir_all(path.join("skills/new_skill")).unwrap();
    fs::write(path.join("skills/new_skill/SKILL.md"), "private content").unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["plugins"]["items"][0]["skills_count"] == 2
    });
    let replacement = path.join("staged.tmp");
    fs::write(&replacement,r#"{"version":"1.0.0","skills":"./skills","mcpServers":{"demo":{},"second":{},"third":{}}}"#).unwrap();
    fs::rename(replacement, path.join("plugin.json")).unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["plugins"]["items"][0]["mcp_count"] == 3
    });
    fs::write(
        home.path().join("config.toml"),
        "[plugins.\"fixture@provider\"]\nenabled = false\n",
    )
    .unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["plugins"]["items"][0]["enabled"] == false
    });
    let set = |enabled| {
        let body = json!({"observations":{"metadata":enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    assert_eq!(set(false).status, 200);
    let disabled = service.get("/api/snapshot").json();
    assert_eq!(disabled["codex"]["plugins"]["health"], "disabled");
    assert_eq!(set(true).status, 200);
    snapshot_when(&service, |value| {
        value["codex"]["plugins"]["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    });
}

#[test]
fn plugin_cache_created_after_start_is_observed() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["plugins"]["health"] == "missing"
    });
    let root = home.path().join("plugins/cache/provider/created/1.0.0");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("plugin.json"), r#"{"version":"1.0.0"}"#).unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["plugins"]["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == "created@provider"))
    });
}

#[test]
fn monitor_snapshot_reports_idle_after_collection() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["monitor"]["collection"]["phase"] == "idle"
    });
    assert_eq!(snapshot["monitor"]["collection"]["phase_ms"], 0);
    assert_eq!(snapshot["monitor"]["collection"]["phase_cpu_ms"], 0);
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
    for phase in ["sessions", "worktrees", "diagnostics", "history"] {
        assert!(record["phase_metrics"][phase]["elapsed_ms"]
            .as_f64()
            .is_some_and(|value| value >= 0.0));
        assert!(record["phase_metrics"][phase]["cpu_ms"]
            .as_f64()
            .is_some_and(|value| value >= 0.0));
    }
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
fn codex_connection_uses_recorded_responses_and_connection_errors() {
    let home = tempdir().unwrap();
    let (path, _) = native_event_fixture(home.path());
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let early = (now - chrono::Duration::seconds(10)).to_rfc3339();
    let later = (now - chrono::Duration::seconds(5)).to_rfc3339();
    let id = "00000000-0000-4000-8000-000000000002";
    fs::write(
        &path,
        [
            event(&early, "session_meta", json!({"id":id})),
            event(
                &early,
                "event_msg",
                json!({"type":"token_count","info":{"total_token_usage":{"input_tokens":1}}}),
            ),
            event(
                &early,
                "event_msg",
                json!({"type":"error","error_code":"stream_disconnected"}),
            ),
        ]
        .concat(),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let first = snapshot_when(&service, |value| {
        value["codex"]["connection"]["state"] == "error"
    });
    assert_eq!(first["codex"]["connection"]["evidence"], "connection_error");
    assert!(first["codex"]["connection"]["last_response_at"].is_string());
    let next = event(
        &later,
        "event_msg",
        json!({"type":"token_count","info":{"total_token_usage":{"input_tokens":2}}}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(next.as_bytes())
        .unwrap();
    let after = snapshot_when(&service, |value| {
        value["codex"]["connection"]["state"] == "response"
    });
    assert_eq!(after["codex"]["connection"]["evidence"], "model_usage");
    let body = json!({"observations":{"codex":false}}).to_string();
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body).status,200);
    assert_eq!(
        service.get("/api/snapshot").json()["codex"]["connection"]["state"],
        "paused"
    );
}

#[test]
fn local_allowance_observation_replays_changes_after_reenable() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let later = (chrono::DateTime::parse_from_rfc3339(&stamp).unwrap()
        + chrono::Duration::seconds(1))
    .to_rfc3339();
    let first = event(
        &stamp,
        "event_msg",
        json!({"type":"token_count","rate_limits":{"primary":{"used_percent":25}}}),
    );
    fs::write(&path, &first).unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["usage"]["limits"][0]["used_percent"] == 25
    });
    let post = |enabled| {
        let body = json!({"observations":{"usage":enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    assert_eq!(post(false).status, 200);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["settings"]["observations"]["usage"], false);
    assert!(disabled["codex"]["usage"].is_null());
    let second = event(
        &later,
        "event_msg",
        json!({"type":"token_count","rate_limits":{"primary":{"used_percent":80}}}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(second.as_bytes())
        .unwrap();
    let scanned = snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|n| n >= (first.len() + second.len()) as u64)
    });
    assert!(scanned["codex"]["usage"].is_null());
    assert_eq!(post(true).status, 200);
    let enabled = snapshot_when(&service, |value| {
        value["codex"]["usage"]["limits"][0]["used_percent"] == 80
    });
    assert_eq!(enabled["settings"]["observations"]["usage"], true);
}

#[test]
fn local_allowance_projects_latest_event_and_restores_checkpoint() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let rows = [
        event(
            &stamp,
            "session_meta",
            json!({"id":"00000000-0000-4000-8000-000000000002"}),
        ),
        event(
            &stamp,
            "event_msg",
            json!({"type":"token_count","rate_limits":{"limit_id":"codex","plan_type":"pro","primary":{"used_percent":25,"window_minutes":300,"resets_at":0},"credits":{"balance":"1.25","secret":"PRIVATE"}}}),
        ),
    ];
    fs::write(&path, rows.concat()).unwrap();
    let mut service = Service::start(home.path(), true, None);
    let initial = snapshot_when(&service, |value| {
        value["codex"]["usage"]["limit_id"] == "codex"
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    assert_eq!(
        initial["codex"]["usage"]["limits"][0]["remaining_percent"],
        75.0
    );
    assert!(!initial.to_string().contains("PRIVATE"));
    service.stop();
    let restored = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&restored, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(snapshot["codex"]["usage"]["limit_id"], "codex");
}

#[test]
fn plain_execution_modes_survive_session_checkpoint_restore() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    fs::write(
        &path,
        [
            event(
                &stamp,
                "session_meta",
                json!({"id":"00000000-0000-4000-8000-000000000002"}),
            ),
            event(
                &stamp,
                "turn_context",
                json!({"sandbox_policy":"workspace-write","collaboration_mode":"default"}),
            ),
        ]
        .concat(),
    )
    .unwrap();
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["execution"]["sandbox_mode"] == "workspace-write")
        }) && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    assert_eq!(
        snapshot["codex"]["threads"][0]["execution"]["collaboration_mode"],
        "default"
    );
    service.stop();
    let mut restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(
        restored["codex"]["threads"][0]["execution"]["sandbox_mode"],
        "workspace-write"
    );
    restarted.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    db.execute("UPDATE session_cursors SET payload=json_remove(json_set(payload,'$.version',16),'$.projection.execution')", []).unwrap();
    drop(db);
    let replayed = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&replayed, |value| {
        value["codex"]["read_state"]["checkpoint"]["invalidated"] == 1
            && value["codex"]["threads"].as_array().is_some_and(|rows| {
                rows.iter()
                    .any(|row| row["execution"]["sandbox_mode"] == "workspace-write")
            })
    });
    assert!(snapshot["codex"]["bytes_read"].as_u64().unwrap() > 0);
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
    fs::write(&path, rows.concat()).unwrap();
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
fn agent_communication_metadata_is_bounded_private_and_replayed_from_old_checkpoint() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let records = [
        event(&stamp, "session_meta", json!({"id":id})),
        event(
            &stamp,
            "response_item",
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"Message Type: MESSAGE\nTask name: /root\nSender: /root/worker\nPayload:\nPRIVATE_MESSAGE"}]}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"message","role":"assistant","channel":"analysis","content":[{"type":"output_text","text":"Message Type: MESSAGE\nTask name: /root\nSender: /root/hidden\nPayload:\nPRIVATE_HIDDEN"}]}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"collaboration.send_message","call_id":"direct","arguments":json!({"target":"/root/worker","message":"PRIVATE_BODY"}).to_string()}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"custom_tool_call","name":"exec","call_id":"nested","input":"await tools.collaboration__spawn_agent({task_name:'review', message:'PRIVATE_NESTED'})"}),
        ),
    ];
    fs::write(path, records.concat()).unwrap();
    let mut service = Service::start(home.path(), true, None);
    let check = |snapshot: &Value| {
        snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["thread_id"] == id
                    && row["agent_messages"]
                        .as_array()
                        .is_some_and(|messages| messages.len() == 3)
            })
        })
    };
    let snapshot = snapshot_when(&service, |value| {
        check(value) && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let messages = snapshot["codex"]["threads"][0]["agent_messages"]
        .as_array()
        .unwrap();
    assert!(messages
        .iter()
        .any(|message| message["direction"] == "incoming" && message["sender"] == "/root/worker"));
    assert!(messages
        .iter()
        .any(|message| message["action"] == "send_message" && message["target"] == "/root/worker"));
    assert!(messages
        .iter()
        .any(|message| message["action"] == "spawn_agent"
            && message["task_name"] == "review"
            && message["nested"] == true));
    assert!(!snapshot.to_string().contains("PRIVATE"));
    service.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String=db.query_row("SELECT group_concat(payload) FROM (SELECT payload FROM session_cursors UNION ALL SELECT payload FROM session_calls)",[],|row|row.get(0)).unwrap();
    assert!(!stored.contains("PRIVATE"));
    drop(db);
    let mut resumed = Service::start(home.path(), true, None);
    let restored = snapshot_when(&resumed, |value| {
        check(value) && value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert!(!restored.to_string().contains("PRIVATE"));
    resumed.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    db.execute_batch("UPDATE session_cursors SET payload=json_remove(json_set(payload,'$.version',5),'$.projection.agent_incoming'); UPDATE session_calls SET payload=json_remove(payload,'$.agent_messages');").unwrap();
    drop(db);
    let restarted = Service::start(home.path(), true, None);
    let replayed = snapshot_when(&restarted, |value| {
        check(value) && value["codex"]["read_state"]["checkpoint"]["invalidated"] == 1
    });
    assert!(replayed["codex"]["bytes_read"].as_u64().unwrap() > 0);
    assert!(!replayed.to_string().contains("PRIVATE"));
}

#[test]
fn context_detail_reads_only_observed_source_and_masks_private_content() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let records = [
        event(&stamp, "session_meta", json!({"id":id})),
        event(
            &stamp,
            "response_item",
            json!({"type":"message","role":"user","channel":{"token":"PRIVATE_CHANNEL","kind":"final"},"content":[{"type":"input_text","text":"Visible api_key=PRIVATE_KEY"}]}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"message","role":"assistant","channel":"analysis","content":[{"type":"output_text","text":"HIDDEN_ANALYSIS"}]}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"reasoning","content":[{"type":"reasoning_text","text":"HIDDEN_REASONING"}],"summary":[{"type":"summary_text","text":"Public summary"}]}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"collaboration.send_message","call_id":"send1","arguments":json!({"target":"/root/worker","message":"PRIVATE_BODY"}).to_string()}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"After call"}]}),
        ),
    ];
    fs::write(&path, records.concat()).unwrap();
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
            && value["codex"]["observed_tool_calls"] == 1
    });
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let route = format!("/api/codex/context?thread_id={id}&call_id=send1");
    let masked = service.get(&route).json();
    assert_eq!(masked["scope"], "preceding_visible_messages");
    assert_eq!(masked["masked"], true);
    assert!(!masked.to_string().contains("PRIVATE_KEY"));
    assert!(!masked.to_string().contains("PRIVATE_CHANNEL"));
    assert!(!masked.to_string().contains("HIDDEN_ANALYSIS"));
    assert!(!masked.to_string().contains("HIDDEN_REASONING"));
    assert!(masked.to_string().contains("Public summary"));
    assert!(!masked.to_string().contains("After call"));
    let unmasked = service.get(&format!("{route}&mask=0")).json();
    assert!(unmasked.to_string().contains("PRIVATE_KEY"));
    assert!(unmasked.to_string().contains("PRIVATE_CHANNEL"));
    assert!(service
        .get(&format!("/api/codex/context?thread_id={id}"))
        .json()
        .to_string()
        .contains("After call"));
    assert_eq!(service.get(&format!("{route}&mask=2")).status, 400);
    assert_eq!(
        service
            .get(&format!(
                "/api/codex/context?thread_id={id}&call_id=unknown"
            ))
            .status,
        409
    );
    service.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored:String=db.query_row("SELECT group_concat(payload) FROM (SELECT payload FROM session_cursors UNION ALL SELECT payload FROM session_calls)",[],|row|row.get(0)).unwrap();
    assert!(!stored.contains("PRIVATE"));
    drop(db);
    let mut restarted = Service::start(home.path(), true, None);
    snapshot_when(&restarted, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert!(restarted
        .get(&route)
        .json()
        .to_string()
        .contains("Public summary"));
    restarted.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    db.execute_batch("UPDATE session_cursors SET payload=json_set(payload,'$.version',6); UPDATE session_calls SET payload=json_remove(payload,'$.request_position','$.response_position');").unwrap();
    drop(db);
    let replayed = Service::start(home.path(), true, None);
    snapshot_when(&replayed, |value| {
        value["codex"]["read_state"]["checkpoint"]["invalidated"] == 1
            && value["codex"]["observed_tool_calls"] == 1
    });
    assert!(replayed
        .get(&route)
        .json()
        .to_string()
        .contains("Public summary"));
}

#[test]
fn context_detail_projects_safe_structures_and_masks_nested_fields() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let body = json!({"type":"message","role":"user","content":[
        {"type":"input_text","text":"name: fixture\nitems:\n  - name: first\n    password: PRIVATE_YAML\n  - name: second\n    value: null"},
        {"type":"input_text","text":"{\"name\":\"one\",\"token\":\"PRIVATE_JSONL\"}\n{\"name\":\"two\"}"},
        {"type":"input_text","text":"{label:'fixture', secret:'PRIVATE_LITERAL', active:true}"},
        {"type":"input_text","text":"{value: run()}"},
        {"type":"input_text","text":"<result><password>PRIVATE_XML</password><count>2</count></result>"},
        {"type":"input_text","text":"password = 'PRIVATE_TOML'\nupdated = 2026-10-05T12:00:00Z"},
        {"type":"input_text","text":"name,password\nexample,PRIVATE_CSV"},
        {"type":"input_text","text":"name\tcount\nexample\t2"},
        {"type":"input_text","text":"token,token\nPRIVATE_DUPLICATE,PRIVATE_DUPLICATE"},
        {"type":"input_text","text":"{'ok': True, 'secret': 'PRIVATE_PYTHON', 'items': (0, False)}"},
        {"type":"input_text","text":"a,b\n\"alpha\", 'beta'"}
    ]});
    fs::write(
        path,
        format!(
            "{}{}",
            event(&stamp, "session_meta", json!({"id":id})),
            event(&stamp, "response_item", body)
        ),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let result = service
        .get(&format!("/api/codex/context?thread_id={id}"))
        .json();
    let content = result["text"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["text"]["items"][1]["value"], Value::Null);
    assert_eq!(content[0]["text"]["items"][0]["password"], "[已隱藏]");
    assert_eq!(content[1]["text"][0]["token"], "[已隱藏]");
    assert_eq!(content[2]["text"]["secret"], "[已隱藏]");
    assert_eq!(content[3]["text"], "{value: run()}");
    assert_eq!(
        content[4]["text"]["result"]["children"][0]["password"],
        "[已隱藏]"
    );
    assert_eq!(content[5]["text"]["password"], "[已隱藏]");
    assert_eq!(content[5]["text"]["updated"], "2026-10-05T12:00:00+00:00");
    assert_eq!(content[6]["text"]["rows"][0]["password"], "[已隱藏]");
    assert_eq!(content[7]["text"]["rows"][0]["count"], "2");
    assert_eq!(content[8]["text"][1][0], "[已隱藏]");
    assert_eq!(content[9]["text"]["secret"], "[已隱藏]");
    assert_eq!(content[9]["text"]["items"], json!([0, false]));
    assert_eq!(content[10]["text"]["rows"][0]["b"], "'beta'");
    assert!(!result.to_string().contains("PRIVATE"));
}

#[test]
fn agent_message_details_select_observed_calls_and_mask_request_and_response() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let records = [
        event(&stamp, "session_meta", json!({"id":id})),
        event(
            &stamp,
            "response_item",
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"Visible context api_key=PRIVATE_CONTEXT"}]}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"collaboration.send_message","call_id":"direct","arguments":json!({"target":"/root/worker","message":"token=PRIVATE_MESSAGE","context":{"password":"PRIVATE_PASSWORD"},"unselected":"PRIVATE_IGNORED"}).to_string()}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output","call_id":"direct","output":{"token":"PRIVATE_RESPONSE","status":"ok"}}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"custom_tool_call","name":"exec","call_id":"nested","input":"await tools.other({}); await tools.collaboration__spawn_agent({task_name:'review', message:'secret=PRIVATE_NESTED'}); await tools.collaboration__send_message({target:'/root/worker', message:'hello'})"}),
        ),
    ];
    fs::write(&path, records.concat()).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["agent_messages"]
                    .as_array()
                    .is_some_and(|messages| messages.len() == 3)
            })
        }) && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let direct = format!("/api/codex/agent-message?thread_id={id}&call_id=direct");
    let masked = service.get(&direct).json();
    assert_eq!(masked["metadata"]["action"], "send_message");
    assert_eq!(masked["context_state"], "recorded");
    assert_eq!(masked["response"]["status"], "ok");
    assert!(!masked.to_string().contains("PRIVATE"));
    assert!(masked["request"].get("unselected").is_none());
    let raw = service.get(&format!("{direct}&mask=0")).json();
    assert!(raw.to_string().contains("PRIVATE_MESSAGE"));
    assert!(raw.to_string().contains("PRIVATE_RESPONSE"));
    let nested = format!("/api/codex/agent-message?thread_id={id}&call_id=nested");
    let first = service.get(&nested).json();
    assert_eq!(first["metadata"]["invocation_index"], 1);
    assert_eq!(first["metadata"]["task_name"], "review");
    assert!(!first.to_string().contains("PRIVATE_NESTED"));
    let second = service.get(&format!("{nested}&index=1")).json();
    assert_eq!(second["metadata"]["action"], "send_message");
    assert_eq!(second["metadata"]["invocation_index"], 2);
    assert_eq!(service.get(&format!("{nested}&index=%D9%A1")).json(), second);
    assert_eq!(service.get(&format!("{nested}&index=2")).status, 409);
    assert_eq!(service.get(&format!("{nested}&index=100")).status, 400);
    assert_eq!(
        service
            .get(&format!("/api/codex/agent-message?thread_id={id}"))
            .status,
        409
    );
}

#[test]
fn tool_detail_is_bounded_masked_and_pages_with_stable_revisions() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let body = "漢".repeat(40000);
    let arguments = json!({"message":body,"api_key":"PRIVATE_KEY"}).to_string();
    let rows = [
        event(&stamp, "session_meta", json!({"id":id})),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"fixture_tool","call_id":"call1","arguments":arguments}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output","call_id":"call1","output":{"status":"ok","token":"PRIVATE_RESPONSE"}}),
        ),
    ];
    fs::write(path, rows.concat()).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["observed_tool_calls"] == 1
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    assert!(!snapshot.to_string().contains("PRIVATE"));
    let base = format!("/api/codex/tool?thread_id={id}&call_id=call1");
    let bounded = service.get(&base).json();
    assert_eq!(bounded["tool"], "fixture_tool");
    assert_eq!(bounded["truncated"], true);
    assert!(!bounded.to_string().contains("PRIVATE"));
    let initial = service.get(&format!("{base}&lazy=1")).json();
    let page = &initial["request"];
    assert_eq!(page["content_page"], true);
    assert_eq!(page["next"], 32768);
    assert!(!initial.to_string().contains("PRIVATE"));
    let revision = page["revision"].as_str().unwrap();
    let next = service
        .get(&format!(
            "{base}&lazy=1&field=request&offset=32768&revision={revision}"
        ))
        .json();
    assert!(next["next"].is_null());
    assert_eq!(next["revision"], revision);
    let unicode_offset: String = form_urlencoded::byte_serialize("٣٢٧٦٨".as_bytes()).collect();
    assert_eq!(service.get(&format!("{base}&lazy=1&field=request&offset={unicode_offset}&revision={revision}")).json(), next);
    assert_eq!(
        service
            .get(&format!(
                "{base}&lazy=1&field=request&offset=32768&revision={revision}&mask=0"
            ))
            .status,
        409
    );
    assert_eq!(
        service
            .get(&format!(
                "{base}&lazy=1&field=output&offset=0&revision={revision}"
            ))
            .status,
        400
    );
    assert_eq!(service.get(&format!("{base}&call_id=other")).status, 400);
    assert_eq!(
        service
            .get(&format!("/api/codex/tool?thread_id={id}&call_id=unknown"))
            .status,
        409
    );
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored:String=db.query_row("SELECT group_concat(payload) FROM (SELECT payload FROM session_cursors UNION ALL SELECT payload FROM session_calls)",[],|row|row.get(0)).unwrap();
    assert!(!stored.contains("PRIVATE"));
}

#[test]
fn tracking_settings_preserve_incremental_all_source_collection() {
    let home = tempdir().unwrap();
    let (_, stamp) = native_event_fixture(home.path());
    let second = home
        .path()
        .join("sessions/rollout-demo-00000000-0000-4000-8000-000000000003.jsonl");
    fs::write(
        second,
        event(
            &stamp,
            "session_meta",
            json!({"id":"00000000-0000-4000-8000-000000000003"}),
        ),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["codex"]["files"] == 2);
    let body = json!({"max_files":1,"track_all":true,"observations":{"files":false}}).to_string();
    let response = service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body);
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["max_files"], 1);
    let snapshot = service.get("/api/snapshot?window=all").json();
    assert_eq!(snapshot["codex"]["files"], 2);
    assert_eq!(snapshot["codex"]["scope"], "incremental_all_sources");
    assert_eq!(snapshot["codex"]["max_files"], 1);
    assert_eq!(snapshot["codex"]["track_all"], true);
    let settings = service.get("/api/settings").json();
    assert_eq!(settings["max_files"], 1);
    assert_eq!(settings["track_all"], true);
    let invalid = r#"{"max_files":0,"track_all":false}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, invalid.len()), invalid).status, 409);
    assert_eq!(service.get("/api/settings").json()["track_all"], true);
    let disabled_home = tempdir().unwrap();
    let disabled = Service::start(disabled_home.path(), false, None);
    let body = r#"{"track_all":true}"#;
    let response = disabled.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", disabled.port, body.len()), body);
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["track_all"], false);
}

#[test]
fn batch_retention_failure_preserves_settings_and_background_finishes_cleanup() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .set_activity_retention(0, std::time::SystemTime::now().into())
        .unwrap();
    drop(store);
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch("WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM ids WHERE n<1001) INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload) SELECT 'activity','web',n,0,'2000-01-01T00:00:00.000Z','{}' FROM ids; CREATE TRIGGER reject_retention BEFORE UPDATE ON history_state WHEN NEW.namespace='activity' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    let mut service = Service::start(home.path(), false, None);
    let post = || {
        let body = json!({"activity_retention_days":1,"debug_mode":true,"observations":{"files":false},"mcp_sources":{"fixture":false}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    assert_eq!(post().status, 503);
    let unchanged = service.get("/api/settings").json();
    assert_eq!(unchanged["activity_retention_days"], 0);
    assert_eq!(unchanged["debug_mode"], false);
    assert_eq!(unchanged["observations"]["files"], true);
    assert!(unchanged["mcp_sources"].get("fixture").is_none());
    let count: i64 = db
        .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1001);
    db.execute_batch("DROP TRIGGER reject_retention").unwrap();
    let applied = post();
    assert_eq!(applied.status, 200);
    let settings = applied.json();
    assert_eq!(settings["activity_retention_days"], 1);
    assert_eq!(settings["debug_mode"], true);
    assert_eq!(settings["observations"]["files"], false);
    assert_eq!(settings["mcp_sources"]["fixture"], false);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let count: i64 = db
            .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
            .unwrap();
        if count == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "background cleanup did not finish"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    service.stop();
}

#[test]
fn batch_settings_validate_observations_before_applying_changes() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    let invalid = post(
        json!({"mcp_sources":{"fixture":false},"observations":{"files":false,"unknown":true},"debug_mode":true}),
    );
    assert_eq!(invalid.status, 409);
    let unchanged = service.get("/api/settings").json();
    assert!(unchanged["mcp_sources"].get("fixture").is_none());
    assert_eq!(unchanged["observations"]["files"], true);
    assert_eq!(unchanged["debug_mode"], false);
    let applied = post(
        json!({"mcp_sources":{"fixture":false},"mcp_descriptions":{"fixture":"Purpose"},"observations":{"metadata":false,"files":false,"mcp":false,"web":true,"logs":false,"tool_events":false},"debug_mode":true}),
    );
    assert_eq!(applied.status, 200);
    let settings = applied.json();
    assert_eq!(settings["mcp_sources"]["fixture"], false);
    assert_eq!(settings["mcp_descriptions"]["fixture"], "Purpose");
    assert_eq!(settings["observations"]["metadata"], false);
    assert_eq!(settings["observations"]["files"], false);
    assert_eq!(settings["observations"]["mcp"], false);
    assert_eq!(settings["observations"]["web"], true);
    assert_eq!(settings["observations"]["logs"], false);
    assert_eq!(settings["observations"]["tool_events"], false);
    assert_eq!(settings["debug_mode"], true);
    assert_eq!(settings["activity_retention_days"], 90);
}

#[test]
fn mcp_customization_batch_replacement_validates_before_changes() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    fs::write(
        home.path().join("config.toml"),
        "[mcp_servers.fixture]\ndescription='Default'\n[mcp_servers.other]\n",
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["mcp"]["configuration"]["configured_sources"] == 2
    });
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    assert_eq!(
        post(json!({"mcp_sources":{"fixture":false},"mcp_descriptions":{"fixture":"Old purpose"}}))
            .status,
        200
    );
    assert_eq!(post(json!({"replace_customizations":true,"mcp_sources":{"fixture":true},"mcp_tags":{"fixture":[" "]}})).status, 409);
    let unchanged = service.get("/api/settings").json();
    assert_eq!(unchanged["mcp_sources"]["fixture"], false);
    assert_eq!(unchanged["mcp_descriptions"]["fixture"], "Old purpose");
    let replaced = post(
        json!({"replace_customizations":true,"mcp_sources":{"other":false},"mcp_categories":{"other":"workspace"},"mcp_descriptions":{"other":"New purpose"}}),
    );
    assert_eq!(replaced.status, 200);
    let settings = replaced.json();
    assert!(settings["mcp_sources"].get("fixture").is_none());
    assert_eq!(settings["mcp_sources"]["other"], false);
    assert!(settings["mcp_descriptions"].get("fixture").is_none());
    assert_eq!(settings["mcp_descriptions"]["other"], "New purpose");
    assert_eq!(settings["activity_retention_days"], 90);
    let snapshot = service.get("/api/snapshot?window=all").json();
    let rows = snapshot["mcp"]["servers"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["server"] == "fixture"
        && row["enabled"] == true
        && row["description"] == "Default"));
    assert!(rows.iter().any(|row| row["server"] == "other"
        && row["enabled"] == false
        && row["description"] == "New purpose"));
}

#[test]
fn mcp_customizations_validate_atomically_and_restore_default_description() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    fs::write(
        home.path().join("config.toml"),
        "[mcp_servers.fixture]\ndescription='Default purpose'\n",
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["mcp"]["configuration"]["configured_sources"] == 1
    });
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    let response = post(
        json!({"mcp_categories":{"fixture":"workspace"},"mcp_descriptions":{"fixture":"Custom purpose"},"mcp_tags":{"fixture":[" tag ","tag","other"]},"tool_descriptions":{"mcp__fixture__read":"Tool purpose"}}),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        response.json()["mcp_tags"]["fixture"],
        json!(["tag", "other"])
    );
    let snapshot = service.get("/api/snapshot?window=all").json();
    let row = snapshot["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["server"] == "fixture")
        .unwrap();
    assert_eq!(row["category"], "workspace");
    assert_eq!(row["description"], "Custom purpose");
    assert_eq!(row["default_description"], "Default purpose");
    assert_eq!(row["tags"], json!(["tag", "other"]));
    assert_eq!(post(json!({"mcp_categories":{"fixture":"invalid"},"mcp_descriptions":{"fixture":"Should not apply"}})).status, 409);
    assert_eq!(
        service.get("/api/settings").json()["mcp_descriptions"]["fixture"],
        "Custom purpose"
    );
    assert_eq!(post(json!({"mcp_descriptions":{"fixture":""}})).status, 200);
    let snapshot = service.get("/api/snapshot?window=all").json();
    let row = snapshot["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["server"] == "fixture")
        .unwrap();
    assert_eq!(row["description"], "Default purpose");
    assert_eq!(
        post(json!({"mcp_sources":{"fixture":false}})).json()["tool_descriptions"]
            ["mcp__fixture__read"],
        "Tool purpose"
    );
}

#[test]
fn session_sql_details_select_recorded_statement_without_persisting_sql() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    let payloads = [
        json!({"type":"function_call","name":"mcp__sqlite__query","call_id":"direct_sql","arguments":{"query":"SELECT 'one;two'; INSERT INTO users(name,password) VALUES ('ok','PRIVATE_SQL')"}}),
        json!({"type":"function_call_output","call_id":"direct_sql","output":{"status":"ok","password":"PRIVATE_OUTPUT"}}),
        json!({"type":"custom_tool_call","name":"exec","call_id":"nested_sql","input":"await tools.mcp__sqlite__query({query:\"SELECT 42\"});"}),
        json!({"type":"function_call","name":"exec_command","call_id":"shell_sql","arguments":{"cmd":"sqlite3 /tmp/demo.db 'SELECT 7'"}}),
    ];
    for payload in payloads {
        file.write_all(event(&stamp, "response_item", payload).as_bytes())
            .unwrap();
    }
    drop(file);
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .filter(|event| event["call_id"] == "direct_sql")
                    .count()
                    == 2
                    && events.iter().any(|event| event["call_id"] == "nested_sql")
                    && events.iter().any(|event| {
                        event["call_id"] == "shell_sql" && event["statement"] == "SELECT"
                    })
            })
    });
    let events = snapshot["codex"]["sqlite"]["events"].as_array().unwrap();
    let find = |call: &str, statement: &str| {
        events
            .iter()
            .find(|event| event["call_id"] == call && event["statement"] == statement)
            .unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let direct = format!("/api/codex/sql?id={}", find("direct_sql", "INSERT"));
    let detail = service.get(&direct).json();
    assert_eq!(detail["content_status"], "available");
    assert_eq!(detail["response_scope"], "containing_tool_call");
    assert!(detail["sql"].as_str().unwrap().contains("[已隱藏]"));
    assert!(!detail.to_string().contains("PRIVATE"));
    assert!(service.get(&format!("{direct}&mask=0")).json()["sql"]
        .as_str()
        .unwrap()
        .contains("PRIVATE_SQL"));
    assert_eq!(
        service.get(&format!("{direct}&lazy=1")).json()["sql"]["content_page"],
        true
    );
    let nested = service
        .get(&format!(
            "/api/codex/sql?id={}",
            find("nested_sql", "SELECT")
        ))
        .json();
    assert_eq!(nested["sql"], "SELECT 42");
    let shell = service
        .get(&format!(
            "/api/codex/sql?id={}",
            find("shell_sql", "SELECT")
        ))
        .json();
    assert_eq!(shell["sql"], "SELECT 7");
    assert!(!snapshot.to_string().contains("PRIVATE_SQL"));
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db.query_row("SELECT group_concat(payload) FROM (SELECT payload FROM session_cursors UNION ALL SELECT payload FROM session_calls)", [], |row| row.get(0)).unwrap();
    assert!(!stored.contains("PRIVATE_SQL"));
}

#[test]
fn diagnostic_sql_details_require_observed_event_and_mask_insert_values() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let sql = "INSERT INTO users(name,password) VALUES ('ok','PRIVATE_SQL')";
    let body = format!(
        "summary={} elapsed=1ms",
        serde_json::to_string(sql).unwrap()
    );
    let db = rusqlite::Connection::open(home.path().join("logs_fixture.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
    )
    .unwrap();
    let timestamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .timestamp_millis() as f64
        / 1000.0;
    db.execute(
        "INSERT INTO logs VALUES(1,?,'INFO','sqlx::query',?)",
        rusqlite::params![timestamp, body],
    )
    .unwrap();
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("codex-desktop-fixture.log"),
        format!("{stamp} INFO [sqlx::query] {body}\n"),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["events"]
            .as_array()
            .is_some_and(|events| {
                events.iter().any(|event| event["source"] == "codex_core")
                    && events
                        .iter()
                        .any(|event| event["source"] == "codex_desktop")
            })
    });
    let locations = snapshot["sources"]["diagnostics"]["locations"].as_array().unwrap();
    assert!(locations.iter().any(|path| path.as_str().is_some_and(|path| path.ends_with("/desktop-logs/codex-desktop-fixture.log"))));
    assert!(locations.iter().any(|path| path.as_str().is_some_and(|path| path.ends_with("/logs_fixture.sqlite"))));
    let events = snapshot["codex"]["sqlite"]["events"].as_array().unwrap();
    for source in ["codex_core", "codex_desktop"] {
        let event = events
            .iter()
            .find(|event| event["source"] == source)
            .unwrap();
        let url = format!("/api/codex/sql?id={}", event["id"].as_str().unwrap());
        let detail = service.get(&url).json();
        let masked = detail["sql"]
            .as_str()
            .unwrap_or_else(|| panic!("missing SQL for {source}: event={event}, detail={detail}"));
        assert!(masked.contains("'ok'") && masked.contains("[已隱藏]"));
        assert!(!masked.contains("PRIVATE_SQL"));
        assert!(service.get(&format!("{url}&mask=0")).json()["sql"]
            .as_str()
            .unwrap()
            .contains("PRIVATE_SQL"));
        assert_eq!(
            service.get(&format!("{url}&lazy=1")).json()["sql"]["content_page"],
            true
        );
    }
    assert!(!snapshot.to_string().contains("PRIVATE_SQL"));
    assert_eq!(
        service
            .get(&format!("/api/codex/sql?id={}", "0".repeat(64)))
            .status,
        409
    );
    assert_eq!(service.get("/api/codex/sql?id=invalid").status, 400);
}

#[test]
fn sqlite_observation_switch_replays_session_and_diagnostics() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    append_records(
        &session,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"mcp__sqlite__query","call_id":"initial_sql","arguments":{"query":"SELECT 1"}}),
        )],
    );
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let desktop = directory.join("codex-desktop-fixture.log");
    fs::write(
        &desktop,
        format!("{stamp} INFO [sqlx::query] sql=\"SELECT 2\" elapsed=2ms\n"),
    )
    .unwrap();
    let source = rusqlite::Connection::open(home.path().join("logs_fixture.sqlite")).unwrap();
    source
        .execute_batch(
            "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
        )
        .unwrap();
    let timestamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .timestamp_millis() as f64
        / 1000.0;
    source
        .execute(
            "INSERT INTO logs VALUES(1,?,'INFO','sqlx::query',?)",
            rusqlite::params![timestamp, "sql=\"SELECT 3\" elapsed=2ms"],
        )
        .unwrap();
    let service = Service::start(home.path(), true, None);
    let initial = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["total"]
            .as_u64()
            .is_some_and(|total| total >= 3)
    });
    let id = initial["codex"]["sqlite"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["call_id"] == "initial_sql")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let set = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    let disabled = set(json!({"observations":{"sqlite":false}}));
    assert_eq!(disabled.status, 200);
    assert_eq!(disabled.json()["observations"]["sqlite"], false);
    assert_eq!(service.get("/api/history?section=sql").status, 400);
    assert_eq!(service.get(&format!("/api/codex/sql?id={id}")).status, 409);
    append_records(
        &session,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"mcp__sqlite__query","call_id":"off_sql","arguments":{"query":"SELECT 4"}}),
        )],
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&desktop)
        .unwrap()
        .write_all(format!("{stamp} ERROR [sqlx::off] sql=\"SELECT 5\" elapsed=2ms\n").as_bytes())
        .unwrap();
    source
        .execute(
            "INSERT INTO logs VALUES(2,?,'ERROR','sqlx::off',?)",
            rusqlite::params![timestamp, "sql=\"SELECT 6\" elapsed=2ms"],
        )
        .unwrap();
    let disabled_snapshot = snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|bytes| bytes >= fs::metadata(&session).unwrap().len())
            && value["errors"]["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["module"] == "sqlx::off" && event["source"] == "codex_core")
                    && events.iter().any(|event| {
                        event["module"] == "sqlx::off" && event["source"] == "codex_desktop"
                    })
            })
    });
    assert_eq!(disabled_snapshot["codex"]["sqlite"]["total"], 0);
    assert_eq!(
        disabled_snapshot["codex"]["sqlite"]["aggregate"]["total"],
        0
    );
    let history = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let count: i64 = history.query_row("SELECT count(*) FROM history_items WHERE namespace='sql' AND (payload LIKE '%off_sql%' OR payload LIKE '%sqlx::off%')", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        set(json!({"observations":{"sqlite":true,"errors":true}})).status,
        200
    );
    snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["events"]
            .as_array()
            .is_some_and(|events| {
                events.iter().any(|event| event["call_id"] == "off_sql")
                    && events.iter().any(|event| {
                        event["module"] == "sqlx::off" && event["source"] == "codex_core"
                    })
                    && events.iter().any(|event| {
                        event["module"] == "sqlx::off" && event["source"] == "codex_desktop"
                    })
            })
    });
    assert_eq!(service.get(&format!("/api/codex/sql?id={id}")).status, 200);
    assert_eq!(service.get("/api/history?section=sql").status, 200);
}

#[test]
fn sqlite_observation_disabled_checkpoint_replays_after_restart() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let body = r#"{"observations":{"sqlite":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), body).status, 200);
    append_records(
        &session,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"custom_tool_call","name":"exec","call_id":"replay_sql","input":"await tools.mcp__sqlite__query({query:\"SELECT 42\"});"}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|bytes| bytes >= fs::metadata(&session).unwrap().len())
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&restarted, |value| {
        value["codex"]["sqlite"]["events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["call_id"] == "replay_sql"))
    });
    assert!(
        snapshot["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn error_observation_disabled_checkpoint_replays_after_restart() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let body = r#"{"observations":{"errors":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), body).status, 200);
    fs::OpenOptions::new()
        .append(true)
        .open(&session)
        .unwrap()
        .write_all(
            event(
                &stamp,
                "event_msg",
                json!({"type":"turn_failed","error_code":"REPLAY_ERROR"}),
            )
            .as_bytes(),
        )
        .unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|bytes| bytes >= fs::metadata(&session).unwrap().len())
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&restarted, |value| {
        value["errors"]["events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["code"] == "REPLAY_ERROR"))
    });
    assert!(
        snapshot["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn error_observation_switch_preserves_logs_and_replays_disabled_records() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let desktop = directory.join("codex-desktop-fixture.log");
    fs::write(
        &desktop,
        format!("{stamp} ERROR [codex] initial errorCode=INITIAL_DESKTOP\n"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(home.path().join("logs_fixture.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
    )
    .unwrap();
    let timestamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .timestamp_millis() as f64
        / 1000.0;
    db.execute(
        "INSERT INTO logs VALUES(1,?,'ERROR','codex_core','initial')",
        [timestamp],
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let initial = snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events.iter().any(|event| event["source"] == "codex_core")
                && events
                    .iter()
                    .any(|event| event["source"] == "codex_desktop")
                && events.iter().any(|event| event["source"] == "session")
        })
    });
    let session_id = initial["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["source"] == "session")
        .unwrap()["content_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let set = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    assert_eq!(set(json!({"observations":{"errors":false}})).status, 200);
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .status,
        400
    );
    assert_eq!(
        service
            .get(&format!("/api/codex/error?id={session_id}"))
            .status,
        409
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&session)
        .unwrap()
        .write_all(
            event(
                &stamp,
                "event_msg",
                json!({"type":"turn_failed","error_code":"OFF_SESSION"}),
            )
            .as_bytes(),
        )
        .unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&desktop)
        .unwrap()
        .write_all(format!("{stamp} ERROR [codex] disabled errorCode=OFF_DESKTOP\n").as_bytes())
        .unwrap();
    db.execute(
        "INSERT INTO logs VALUES(2,?,'ERROR','codex_core::unique','disabled')",
        [timestamp],
    )
    .unwrap();
    let deadline = Instant::now() + TIMEOUT;
    let logs = loop {
        let value = service.get("/api/logs?window=all").json();
        if value["entries"].as_array().is_some_and(|entries| {
            entries.iter().any(|event| event["code"] == "OFF_DESKTOP")
                && entries
                    .iter()
                    .any(|event| event["module"] == "codex_core::unique")
        }) {
            break value;
        }
        assert!(Instant::now() < deadline, "diagnostic logs did not update");
        std::thread::sleep(Duration::from_millis(20));
    };
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["errors"]["enabled"], false);
    assert_eq!(disabled["errors"]["total"], 0);
    assert_eq!(disabled["errors"]["history"]["retained"], 0);
    let core_id = logs["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["module"] == "codex_core::unique")
        .unwrap()["content_id"]
        .as_str()
        .unwrap();
    assert_eq!(
        service
            .get(&format!("/api/codex/error?id={core_id}"))
            .status,
        200
    );
    let history = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let new_errors: i64 = history.query_row("SELECT count(*) FROM history_items WHERE namespace='errors' AND (payload LIKE '%OFF_SESSION%' OR payload LIKE '%OFF_DESKTOP%' OR payload LIKE '%codex_core::unique%')", [], |row| row.get(0)).unwrap();
    assert_eq!(new_errors, 0);
    drop(history);
    assert_eq!(set(json!({"observations":{"errors":true}})).status, 200);
    snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events.iter().any(|event| event["code"] == "OFF_SESSION")
                && events.iter().any(|event| event["code"] == "OFF_DESKTOP")
                && events
                    .iter()
                    .any(|event| event["module"] == "codex_core::unique")
        })
    });
    assert_eq!(
        set(json!({"observations":{"errors":false,"logs":false}})).status,
        200
    );
    assert_eq!(
        service
            .get(&format!("/api/codex/error?id={core_id}"))
            .status,
        409
    );
}

#[test]
fn error_detail_can_read_observed_event_beyond_summary_when_logs_are_disabled() {
    let home = tempdir().unwrap();
    let core = rusqlite::Connection::open(home.path().join("logs_fixture.sqlite")).unwrap();
    core.execute_batch("CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)").unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    for id in 1..=600 {
        core.execute("INSERT INTO logs VALUES(?1,?2,'ERROR','codex_core',?3)", rusqlite::params![id, (now - chrono::Duration::hours(2) + chrono::Duration::seconds(id)).timestamp() as f64, "Observed old password=PRIVATE_OLD"]).unwrap();
    }
    drop(core);
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let mut desktop = String::new();
    for index in 0..600 {
        let stamp = (now - chrono::Duration::hours(1) + chrono::Duration::seconds(index)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        desktop.push_str(&format!("{stamp} ERROR [codex] recent index={index}\n"));
    }
    fs::write(directory.join("codex-desktop-fixture.log"), desktop).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["errors"]["total"].as_u64().is_some_and(|total| total >= 1200));
    let shown = snapshot["errors"]["events"].as_array().unwrap();
    assert_eq!(shown.len(), 1000);
    let logs = service.get("/api/logs?window=all").json();
    let old = logs["entries"].as_array().unwrap().iter().find(|event| event["source"] == "codex_core" && event["record_id"] == 1).unwrap();
    let id = old["content_id"].as_str().unwrap();
    assert!(!shown.iter().any(|event| event["content_id"] == id));
    let body = r#"{"observations":{"logs":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), body).status, 200);
    let detail = service.get(&format!("/api/codex/error?id={id}")).json();
    assert_eq!(detail["content_status"], "available");
    assert!(detail["text"].as_str().unwrap().contains("Observed old"));
    assert!(!detail.to_string().contains("PRIVATE_OLD"));
}

#[test]
fn desktop_error_detail_reads_observed_line_without_exposing_private_data() {
    let home = tempdir().unwrap();
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(
        directory.join("codex-desktop-fixture.log"),
        format!("{stamp} ERROR [codex] Observed desktop password=PRIVATE_DESKTOP\n"),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["source"] == "codex_desktop")
        })
    });
    let event = snapshot["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["source"] == "codex_desktop")
        .unwrap();
    let url = format!(
        "/api/codex/error?id={}",
        event["content_id"].as_str().unwrap()
    );
    let detail = service.get(&url).json();
    assert_eq!(detail["content_status"], "available");
    assert!(detail["text"]
        .as_str()
        .unwrap()
        .contains("Observed desktop"));
    assert!(!detail.to_string().contains("PRIVATE_DESKTOP"));
    assert!(service.get(&format!("{url}&mask=0")).json()["text"]
        .as_str()
        .unwrap()
        .contains("PRIVATE_DESKTOP"));
    assert_eq!(
        service.get(&format!("{url}&lazy=1")).json()["text"]["content_page"],
        true
    );
    assert!(!snapshot.to_string().contains("PRIVATE_DESKTOP"));
}

#[test]
fn core_error_detail_reads_only_observed_record_and_masks_content() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let path = home.path().join("logs_fixture.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT,feedback_log_body TEXT)").unwrap();
    let timestamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .timestamp_millis() as f64
        / 1000.0;
    db.execute(
        "INSERT INTO logs VALUES(1,?,'WARN','codex_core',?,NULL)",
        rusqlite::params![timestamp, "Observed diagnostic password=PRIVATE_DIAGNOSTIC"],
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["source"] == "codex_core" && event["record_id"] == 1)
        })
    });
    let event = snapshot["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["source"] == "codex_core" && event["record_id"] == 1)
        .unwrap();
    let id = event["content_id"].as_str().unwrap();
    let url = format!("/api/codex/error?id={id}");
    let detail = service.get(&url).json();
    assert_eq!(detail["content_status"], "available");
    assert!(detail["text"]
        .as_str()
        .unwrap()
        .contains("Observed diagnostic"));
    assert!(!detail.to_string().contains("PRIVATE_DIAGNOSTIC"));
    assert!(service.get(&format!("{url}&mask=0")).json()["text"]
        .as_str()
        .unwrap()
        .contains("PRIVATE_DIAGNOSTIC"));
    assert_eq!(
        service.get(&format!("{url}&lazy=1")).json()["text"]["content_page"],
        true
    );
    assert!(!snapshot.to_string().contains("PRIVATE_DIAGNOSTIC"));
    assert_eq!(
        service
            .get(&format!("/api/codex/error?id={}", "0".repeat(64)))
            .status,
        409
    );
}

#[test]
fn session_and_tool_error_details_require_observed_identity_and_mask_content() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    let payloads = [
        (
            "event_msg",
            json!({"type":"turn_failed","message":"Session message","error":{"code":"failed","password":"PRIVATE_SESSION"}}),
        ),
        (
            "response_item",
            json!({"type":"function_call","name":"fixture_tool","call_id":"failed_call","arguments":"{}"}),
        ),
        (
            "response_item",
            json!({"type":"function_call_output","call_id":"failed_call","output":{"isError":true,"status":"failed","api_key":"PRIVATE_TOOL"}}),
        ),
    ];
    for (kind, payload) in payloads {
        file.write_all(event(&stamp, kind, payload).as_bytes())
            .unwrap();
    }
    drop(file);
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events.iter().any(|event| event["reason"] == "turn_failed")
                && events.iter().any(|event| event["call_id"] == "failed_call")
        })
    });
    let events = snapshot["errors"]["events"].as_array().unwrap();
    let session = events
        .iter()
        .find(|event| event["reason"] == "turn_failed")
        .unwrap()["content_id"]
        .as_str()
        .unwrap();
    let tool = events
        .iter()
        .find(|event| event["call_id"] == "failed_call")
        .unwrap()["content_id"]
        .as_str()
        .unwrap();
    let session_url = format!("/api/codex/error?id={session}");
    let detail = service.get(&session_url).json();
    assert_eq!(detail["content_status"], "available");
    assert_eq!(detail["text"]["message"], "Session message");
    assert!(!detail.to_string().contains("PRIVATE_SESSION"));
    assert!(service
        .get(&format!("{session_url}&mask=0"))
        .json()
        .to_string()
        .contains("PRIVATE_SESSION"));
    let paged = service.get(&format!("{session_url}&lazy=1")).json();
    assert_eq!(paged["text"]["content_page"], true);
    let tool_detail = service.get(&format!("/api/codex/error?id={tool}")).json();
    assert_eq!(tool_detail["content_status"], "available");
    assert!(!tool_detail.to_string().contains("PRIVATE_TOOL"));
    assert_eq!(service.get("/api/codex/error?id=invalid").status, 400);
    assert_eq!(
        service
            .get(&format!("/api/codex/error?id={}", "0".repeat(64)))
            .status,
        409
    );
}

#[test]
fn mcp_catalog_extracts_python_module_docstring_without_execution() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let real_home = home.path().canonicalize().unwrap();
    let runtime = real_home.join("venv/bin/python");
    let module = real_home.join("venv/lib/python3.14/site-packages/example_pkg");
    fs::create_dir_all(runtime.parent().unwrap()).unwrap();
    fs::create_dir_all(&module).unwrap();
    fs::write(&runtime, "not executable").unwrap();
    fs::write(module.join("runner.py"), "\"\"\"Local module purpose api_key=PRIVATE_KEY\"\"\"\nraise RuntimeError('PRIVATE_CODE')\n").unwrap();
    let runtime = serde_json::to_string(&runtime.to_string_lossy()).unwrap();
    fs::write(
        home.path().join("config.toml"),
        format!("[mcp_servers.example]\ncommand={runtime}\nargs=['-m','example_pkg.runner']\n"),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["mcp"]["servers"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["server"] == "example" && row["description_source"] == "runner.py")
        })
    });
    let row = snapshot["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["server"] == "example")
        .unwrap();
    assert!(row["description"]
        .as_str()
        .unwrap()
        .contains("Local module purpose"));
    assert!(!snapshot.to_string().contains("PRIVATE_KEY"));
    assert!(!snapshot.to_string().contains("PRIVATE_CODE"));
}

#[test]
fn mcp_catalog_reads_bounded_package_and_readme_introductions() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let real_home = home.path().canonicalize().unwrap();
    let package = real_home.join("package_fixture");
    let readme = real_home.join("readme_fixture");
    fs::create_dir(&package).unwrap();
    fs::create_dir(&readme).unwrap();
    fs::write(package.join("server.py"), "PRIVATE_SCRIPT").unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"description":"Package purpose api_key=PRIVATE_KEY"}"#,
    )
    .unwrap();
    fs::write(
        package.join("README.md"),
        "# Title\n\nNot the selected purpose\n",
    )
    .unwrap();
    fs::write(readme.join("server.py"), "PRIVATE_SCRIPT").unwrap();
    fs::write(
        readme.join("README.md"),
        "# Title\n\nProvides [local indexes](https://example.invalid)\n\n```\nPRIVATE_CODE\n```\n",
    )
    .unwrap();
    let package_script =
        serde_json::to_string(&package.join("server.py").to_string_lossy()).unwrap();
    let readme_script = serde_json::to_string(&readme.join("server.py").to_string_lossy()).unwrap();
    fs::write(home.path().join("config.toml"), format!("[mcp_servers.package]\nargs=[{package_script}]\n[mcp_servers.readme]\nargs=[{readme_script}]\n")).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["mcp"]["configuration"]["configured_sources"] == 2
    });
    let rows = snapshot["mcp"]["servers"].as_array().unwrap();
    let package = rows.iter().find(|row| row["server"] == "package").unwrap();
    let readme = rows.iter().find(|row| row["server"] == "readme").unwrap();
    assert_eq!(package["description_source"], "package.json");
    assert!(package["description"]
        .as_str()
        .unwrap()
        .contains("Package purpose"));
    assert_eq!(readme["description_source"], "README.md");
    assert_eq!(readme["description"], "Provides local indexes");
    assert!(!snapshot.to_string().contains("PRIVATE_KEY"));
    assert!(!snapshot.to_string().contains("PRIVATE_SCRIPT"));
    assert!(!snapshot.to_string().contains("PRIVATE_CODE"));
}

#[test]
fn configured_mcp_catalog_updates_without_codex_collection() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let initial = service.get("/api/snapshot?window=all").json();
    assert_eq!(initial["codex"]["health"], "disabled");
    fs::write(
        home.path().join("config.toml"),
        "[mcp_servers.fixture]\ndescription='Local purpose'\n",
    )
    .unwrap();
    let updated = snapshot_when(&service, |value| {
        value["mcp"]["servers"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["server"] == "fixture"))
    });
    assert_eq!(updated["codex"]["health"], "disabled");
    assert_eq!(updated["mcp"]["configuration"]["configured_sources"], 1);
}

#[test]
fn configured_mcp_catalog_omits_launch_configuration_and_updates() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let config = home.path().join("config.toml");
    fs::write(&config, "[mcp_servers.fixture]\ndescription='Fixture description api_key=PRIVATE_DESCRIPTION'\ncommand='PRIVATE_COMMAND'\n[mcp_servers.fixture.env]\nTOKEN='PRIVATE_ENV'\n[mcp_servers.disabled]\nenabled=false\n[plugins.\"code-review@openai-bundled\"]\nenabled=true\n").unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["mcp"]["configuration"]["configured_sources"] == 2
    });
    let rows = snapshot["mcp"]["servers"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["server"] == "fixture"
        && row["origin"] == "configured"
        && row["description"]
            .as_str()
            .is_some_and(|text| text.contains("Fixture description")
                && !text.contains("PRIVATE_DESCRIPTION"))));
    assert!(rows.iter().any(|row| row["server"] == "code_review"));
    assert!(!snapshot.to_string().contains("PRIVATE_COMMAND"));
    assert!(!snapshot.to_string().contains("PRIVATE_ENV"));
    assert!(!snapshot.to_string().contains("PRIVATE_DESCRIPTION"));
    let scoped = service.get("/api/snapshot?window=1h").json();
    assert_eq!(scoped["mcp"]["configuration"]["configured_sources"], 2);
    fs::write(
        &config,
        "[mcp_servers.next]\ndescription='Updated source'\n",
    )
    .unwrap();
    let updated = snapshot_when(&service, |value| {
        value["mcp"]["servers"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["server"] == "next"))
    });
    assert_eq!(updated["mcp"]["configuration"]["configured_sources"], 1);
    assert!(!updated["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["server"] == "fixture"));
}

#[test]
fn mcp_and_web_observation_switches_filter_independently_and_replay() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    for (call, tool) in [
        ("mcp_call", "mcp__workspace_inspection__read"),
        ("web_call", "web__run"),
    ] {
        file.write_all(event(&stamp, "response_item", json!({"type":"function_call","name":tool,"call_id":call,"arguments":{"max_files":12}})).as_bytes()).unwrap();
    }
    drop(file);
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["mcp"]["total_events"] == 2);
    let set_observation = |name: &str, enabled: bool| {
        let body = json!({"observations":{(name):enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    assert_eq!(set_observation("mcp", false).status, 200);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["settings"]["observations"]["mcp"], false);
    assert_eq!(disabled["mcp"]["total_events"], 1);
    assert_eq!(disabled["mcp"]["events"][0]["server"], "web");
    assert_eq!(
        service
            .get("/api/history?namespace=activity&section=mcp")
            .status,
        400
    );
    assert_eq!(
        service
            .get(&format!(
                "/api/codex/mcp?thread_id={id}&call_id=mcp_call&index=0"
            ))
            .status,
        409
    );
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__workspace_inspection__read","call_id":"while_off","arguments":{"max_files":444}})).as_bytes()).unwrap();
    snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 3);
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='while_off'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains("444"));
    drop(db);
    assert_eq!(set_observation("mcp", true).status, 200);
    snapshot_when(&service, |value| {
        value["mcp"]["events"].as_array().is_some_and(|events| {
            events.iter().any(|event| {
                event["call_id"] == "while_off" && event["metadata"]["max_files"] == 444
            })
        })
    });
    assert_eq!(set_observation("web", false).status, 200);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["settings"]["observations"]["web"], false);
    assert!(disabled["mcp"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|event| event["server"] != "web"));
    assert_eq!(
        service
            .get("/api/history?namespace=activity&section=web")
            .status,
        400
    );
    assert_eq!(
        service
            .get(&format!(
                "/api/codex/mcp?thread_id={id}&call_id=web_call&index=0"
            ))
            .status,
        409
    );
    assert_eq!(set_observation("web", true).status, 200);
    snapshot_when(&service, |value| {
        value["mcp"]["events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["call_id"] == "web_call"))
    });
}

#[test]
fn mcp_source_disabled_checkpoint_replays_after_restart() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let body = json!({"mcp_sources":{"workspace_inspection":false}}).to_string();
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body).status, 200);
    fs::OpenOptions::new().append(true).open(path).unwrap().write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__workspace_inspection__read","call_id":"restart_source","arguments":{"max_files":321}})).as_bytes()).unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["observed_tool_calls"] == 1
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["mcp"]["events"].as_array().is_some_and(|events| {
            events.iter().any(|event| {
                event["call_id"] == "restart_source" && event["metadata"]["max_files"] == 321
            })
        })
    });
    assert!(
        restored["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn mcp_source_setting_filters_history_details_and_replays_on_enable() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    for (call, tool) in [
        ("a", "mcp__workspace_inspection__read"),
        ("b", "mcp__other__read"),
    ] {
        file.write_all(event(&stamp, "response_item", json!({"type":"function_call","name":tool,"call_id":call,"arguments":{"max_files":1}})).as_bytes()).unwrap();
    }
    drop(file);
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["mcp"]["total_events"] == 2);
    let set_source = |enabled: bool| {
        let body = json!({"mcp_sources":{"workspace_inspection":enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    assert_eq!(set_source(false).status, 200);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(
        disabled["settings"]["mcp_sources"]["workspace_inspection"],
        false
    );
    assert_eq!(disabled["mcp"]["total_events"], 1);
    assert_eq!(disabled["mcp"]["events"][0]["server"], "other");
    assert!(disabled["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["server"] == "workspace_inspection" && row["enabled"] == false));
    let disabled_source = disabled["mcp"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["server"] == "workspace_inspection")
        .unwrap();
    assert_eq!(disabled_source["connection"]["state"], "paused");
    assert_eq!(
        service
            .get(&format!("/api/codex/mcp?thread_id={id}&call_id=a&index=0"))
            .status,
        409
    );
    let history = service
        .get("/api/history?namespace=activity&section=mcp")
        .json();
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["server"] != "workspace_inspection"));
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__workspace_inspection__inspect","call_id":"disabled","arguments":{"max_files":777}})).as_bytes()).unwrap();
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(event(&stamp, "response_item", json!({"type":"function_call_output","call_id":"disabled","output":{"isError":true,"status":"failed"}})).as_bytes()).unwrap();
    let disabled = snapshot_when(&service, |value| {
        value["codex"]["observed_tool_calls"] == 3
            && value["codex"]["threads"][0]["tool_events"]
                .as_array()
                .is_some_and(|events| {
                    events.iter().any(|event| {
                        event["call_id"] == "disabled" && event["completed_at"].is_string()
                    })
                })
    });
    assert!(!disabled["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "disabled"));
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='disabled'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains("777"));
    drop(db);
    assert_eq!(set_source(true).status, 200);
    let enabled = snapshot_when(&service, |value| value["mcp"]["total_events"] == 3);
    assert!(enabled["mcp"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "disabled" && event["metadata"]["max_files"] == 777));
    assert!(enabled["errors"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["call_id"] == "disabled"));
    assert_eq!(
        service
            .get(&format!("/api/codex/mcp?thread_id={id}&call_id=a&index=0"))
            .status,
        200
    );
}

#[test]
fn mcp_detail_requires_observed_index_and_masks_static_payloads() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    let payloads = [
        json!({"type":"function_call","name":"mcp__fixture__read_file","call_id":"direct","arguments":{"path":"example","api_key":"PRIVATE_REQUEST"}}),
        json!({"type":"function_call_output","call_id":"direct","output":{"result":"ok","password":"PRIVATE_RESPONSE"}}),
        json!({"type":"custom_tool_call","name":"exec","call_id":"nested","input":"await tools.mcp__fixture__first({count:1}); await tools.mcp__fixture__second({count:2,password:'PRIVATE_NESTED'});"}),
        json!({"type":"custom_tool_call_output","call_id":"nested","output":{"status":"ok"}}),
        json!({"type":"custom_tool_call","name":"exec","call_id":"dynamic","input":"await tools.mcp__fixture__dynamic({filePath: file, password:'PRIVATE_DYNAMIC'});"}),
    ];
    for payload in payloads {
        file.write_all(event(&stamp, "response_item", payload).as_bytes())
            .unwrap();
    }
    drop(file);
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["mcp"]["total_events"] == 4);
    let direct = format!("/api/codex/mcp?thread_id={id}&call_id=direct&index=0");
    let detail = service.get(&direct).json();
    assert_eq!(detail["request"]["path"], "example");
    assert_eq!(service.get(&direct.replace("index=0", "index=%D9%A0")).json(), detail);
    assert_eq!(detail["response_scope"], "mcp_tool_call");
    assert!(!detail.to_string().contains("PRIVATE"));
    let dynamic = format!("/api/codex/mcp?thread_id={id}&call_id=dynamic&index=0");
    let masked = service.get(&dynamic).json();
    assert_eq!(masked["request_source"], "recorded_expression");
    assert!(!masked.to_string().contains("PRIVATE_DYNAMIC"));
    let unmasked = service.get(&format!("{dynamic}&mask=0")).json();
    assert!(unmasked["request"]
        .as_str()
        .unwrap()
        .contains("filePath: file"));
    assert!(unmasked["request"]
        .as_str()
        .unwrap()
        .contains("PRIVATE_DYNAMIC"));
    let nested = format!("/api/codex/mcp?thread_id={id}&call_id=nested&index=1");
    let detail = service.get(&format!("{nested}&lazy=1")).json();
    assert_eq!(detail["response_scope"], "containing_tool_call");
    assert!(detail["request"]["text"]
        .as_str()
        .unwrap()
        .contains("count"));
    assert!(!detail.to_string().contains("PRIVATE"));
    assert_eq!(
        service
            .get(&format!(
                "/api/codex/mcp?thread_id={id}&call_id=nested&index=9"
            ))
            .status,
        409
    );
    assert_eq!(service.get(&format!("{direct}&index=1")).status, 400);
}

#[test]
fn command_details_select_observed_parts_and_label_outer_output() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    let payloads = [
        json!({"type":"function_call","name":"exec_command","call_id":"gitcall","arguments":{"cmd":"git status; echo OTHER_PRIVATE"}}),
        json!({"type":"function_call_output","call_id":"gitcall","output":{"status":"ok","api_key":"PRIVATE_KEY"}}),
        json!({"type":"custom_tool_call","name":"exec","call_id":"checkcall","input":"await tools.exec_command({cmd:'cargo test'});"}),
        json!({"type":"custom_tool_call_output","call_id":"checkcall","output":{"status":"ok"}}),
    ];
    for payload in payloads {
        file.write_all(event(&stamp, "response_item", payload).as_bytes())
            .unwrap();
    }
    drop(file);
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 2);
    let git = format!("/api/codex/git?thread_id={id}&call_id=gitcall&operation=status");
    let detail = service.get(&git).json();
    assert_eq!(detail["commands"], json!(["git status"]));
    assert_eq!(detail["output_scope"], "outer_scope");
    assert_eq!(detail["result_scope"], "containing_tool_call");
    assert!(!detail.to_string().contains("PRIVATE_KEY"));
    let lazy = service.get(&format!("{git}&lazy=1")).json();
    assert_eq!(lazy["request"]["content_page"], true);
    assert_eq!(lazy["output"]["content_page"], true);
    let check = form_urlencoded::Serializer::new(String::new())
        .append_pair("thread_id", id)
        .append_pair("call_id", "checkcall")
        .append_pair("operation", "cargo test")
        .append_pair("lazy", "1")
        .finish();
    let detail = service.get(&format!("/api/codex/check?{check}")).json();
    assert_eq!(detail["commands"], json!(["cargo test"]));
    assert_eq!(detail["output_scope"], "outer_scope");
    assert_eq!(service.get(&format!("{git}&operation=reset")).status, 400);
}

#[test]
#[cfg(unix)]
fn account_observation_schedules_only_when_enabled_and_publishes_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    let executable = bin.path().join("codex");
    let script = r#"#!/bin/sh
printf x >> "$CODEX_HOME/fake-count"
while IFS= read -r line; do
case "$line" in
*'"id":0'*) printf '%s\n' '{"id":0,"result":{}}' ;;
*'"id":1'*) printf '%s\n' '{"id":1,"result":{"account":{"planType":"pro","email":"PRIVATE_EMAIL"}}}' ;;
*'"id":2'*) printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":25}}}}' ;;
*'"id":3'*) printf '%s\n' '{"id":3,"result":{"summary":{"lifetimeTokens":12}}}' ;;
esac
done
"#;
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::start_with_path(home.path(), false, None, Some(bin.path()));
    let initial = service.get("/api/snapshot?window=all").json();
    assert_eq!(initial["settings"]["observations"]["codex_account"], false);
    assert_eq!(initial["settings"]["api_intervals"]["codex_account"], 300);
    assert!(!home.path().join("fake-count").exists());
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    assert_eq!(
        post(json!({"observations":{"codex_account":true},"api_intervals":{"codex_account":59}}))
            .status,
        409
    );
    assert!(!home.path().join("fake-count").exists());
    assert_eq!(
        post(json!({"observations":{"codex_account":true},"api_intervals":{"codex_account":60}}))
            .status,
        200
    );
    let snapshot = snapshot_when(&service, |value| {
        value["api_queries"]["codex_account"]["health"] == "ok"
    });
    assert_eq!(snapshot["codex"]["account"]["plan_type"], "pro");
    assert_eq!(
        snapshot["codex"]["usage"]["limits"][0]["remaining_percent"],
        75
    );
    assert_eq!(snapshot["sources"]["account_api"]["health"], "ok");
    assert!(!snapshot.to_string().contains("PRIVATE_EMAIL"));
    assert_eq!(service.get("/api/codex/account").json()["health"], "ok");
    assert_eq!(
        fs::read_to_string(home.path().join("fake-count")).unwrap(),
        "x"
    );
    assert_eq!(
        post(json!({"observations":{"codex_account":false}})).status,
        200
    );
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["settings"]["observations"]["codex_account"], false);
    assert_eq!(disabled["api_queries"]["codex_account"]["health"], "ok");
}

#[test]
#[cfg(unix)]
fn account_endpoint_uses_explicit_fake_codex_and_caches_safe_projection() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    let executable = bin.path().join("codex");
    let script = r#"#!/bin/sh
printf x >> "$CODEX_HOME/fake-count"
while IFS= read -r line; do
printf '%s\n' "$line" >> "$CODEX_HOME/fake-requests"
case "$line" in
*'"id":0'*) printf '%s\n' '{"id":0,"result":{}}' ;;
*'"id":1'*) printf '%s\n' '{"id":1,"result":{"account":{"planType":"pro","email":"PRIVATE_EMAIL"}}}' ;;
*'"id":2'*) printf '%s\n' '{"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":0}}}}' ;;
*'"id":3'*) printf '%s\n' '{"id":3,"result":{"summary":{"lifetimeTokens":0}}}' ;;
esac
done
"#;
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::start_with_path(home.path(), false, None, Some(bin.path()));
    assert!(!home.path().join("fake-count").exists());
    let first = service.get("/api/codex/account").json();
    assert_eq!(first["health"], "ok");
    assert_eq!(first["plan_type"], "pro");
    assert!(!first.to_string().contains("PRIVATE_EMAIL"));
    assert_eq!(service.get("/api/codex/account").json()["health"], "ok");
    assert_eq!(
        fs::read_to_string(home.path().join("fake-count")).unwrap(),
        "x"
    );
    let requests = fs::read_to_string(home.path().join("fake-requests")).unwrap();
    let messages: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        messages
            .iter()
            .map(|message| message["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "initialize",
            "initialized",
            "account/read",
            "account/rateLimits/read",
            "account/usage/read"
        ]
    );
    assert_eq!(messages[2]["params"]["refreshToken"], false);
}

#[test]
fn public_query_settings_are_disabled_and_invalid_sources_are_rejected() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let snapshot = service.get("/api/snapshot?window=all").json();
    for key in ["public_openai", "public_codex"] {
        assert_eq!(snapshot["settings"]["observations"][key], false);
        assert_eq!(snapshot["settings"]["api_intervals"][key], 300);
    }
    assert_eq!(service.get("/api/public-query?source=other").status, 400);
    assert_eq!(
        service
            .get("/api/public-query?source=openai&source=codex")
            .status,
        400
    );
    let body = json!({"api_intervals":{"public_openai":120,"public_codex":240}}).to_string();
    let reply = service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["api_intervals"]["public_codex"], 240);
}

#[test]
fn legacy_interval_settings_are_validated_without_changing_effective_settings() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    for value in [
        json!({"interval":1}),
        json!({"interval":3600,"idle_minutes":0}),
        json!({"idle_minutes":1440}),
    ] {
        let reply = post(value);
        assert_eq!(reply.status, 200);
        assert!(reply.json().get("interval").is_none());
    }
    for value in [
        json!({"interval":0,"max_files":50}),
        json!({"interval":3601}),
        json!({"interval":true}),
        json!({"idle_minutes":1441}),
        json!({"idle_minutes":-1}),
    ] {
        assert_eq!(post(value).status, 409);
    }
    assert_eq!(
        service.get("/api/snapshot?window=all").json()["settings"]["max_files"],
        20
    );
}

#[test]
fn default_settings_restore_supported_settings_without_changing_codex_mode() {
    for codex in [false, true] {
        let home = tempdir().unwrap();
        let service = Service::start(home.path(), codex, None);
        let snapshot = service.get("/api/snapshot?window=all").json();
        let defaults = &snapshot["default_settings"];
        assert_eq!(defaults["observations"]["codex"], codex);
        assert_eq!(defaults["activity_retention_days"], 90);
        assert_eq!(defaults["api_intervals"]["platform_costs"], 300);
        let post = |value: Value| {
            let body = value.to_string();
            service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
        };
        assert_eq!(
            post(json!({"max_files":50,"observations":{"usage":false}})).status,
            200
        );
        let mut reset = defaults.clone();
        reset["replace_customizations"] = json!(true);
        let reply = post(reset);
        assert_eq!(reply.status, 200);
        let settings = reply.json();
        assert_eq!(settings["max_files"], 20);
        assert_eq!(settings["observations"]["usage"], true);
        assert_eq!(
            post(json!({"observations":{"codex":!codex},"max_files":30})).status,
            200
        );
    }
}

#[test]
fn codex_observation_switch_hides_and_reloads_sessions() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    fs::write(
        &path,
        [
            event(&stamp, "session_meta", json!({"id":id})),
            event(&stamp, "turn_context", json!({"model":"before-disable"})),
        ]
        .concat(),
    )
    .unwrap();
    let service = Service::start(home.path(), false, None);
    let post = |enabled| {
        let body = json!({"observations":{"codex":enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    assert_eq!(
        service.get("/api/snapshot?window=all").json()["codex"]["health"],
        "disabled"
    );
    assert_eq!(post(true).status, 200);
    let initial = snapshot_when(&service, |value| {
        value["codex"]["threads"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["model"] == "before-disable"))
    });
    assert_eq!(initial["settings"]["observations"]["codex"], true);
    let body = json!({"debug_mode":false}).to_string();
    let response = service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body);
    assert_eq!(response.json()["observations"]["codex"], true);
    assert_eq!(post(false).status, 200);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["codex"]["health"], "disabled");
    assert_eq!(disabled["codex"]["threads"], json!([]));
    assert_eq!(disabled["settings"]["observations"]["codex"], false);
    let later = (chrono::DateTime::parse_from_rfc3339(&stamp).unwrap()
        + chrono::Duration::seconds(1))
    .to_rfc3339();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(event(&later, "turn_context", json!({"model":"after-disable"})).as_bytes())
        .unwrap();
    assert_eq!(post(true).status, 200);
    snapshot_when(&service, |value| {
        value["codex"]["threads"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["model"] == "after-disable"))
    });
}

#[test]
fn platform_settings_default_to_disabled_and_reject_invalid_sources() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let snapshot = service.get("/api/snapshot?window=all").json();
    for key in [
        "platform_models",
        "platform_usage",
        "platform_decisions",
        "platform_costs",
    ] {
        assert_eq!(snapshot["settings"]["observations"][key], false);
        assert_eq!(snapshot["settings"]["api_intervals"][key], 300);
    }
    assert!(snapshot["platform_credentials"]["available"]["models"].is_boolean());
    assert_eq!(service.get("/api/openai-platform?source=other").status, 400);
    let body = json!({"api_intervals":{"platform_models":120,"platform_costs":240}}).to_string();
    let reply = service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["api_intervals"]["platform_costs"], 240);
}

#[cfg(unix)]
#[test]
fn account_and_cloud_reject_get_bodies_before_starting_codex() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    let executable = bin.path().join("codex");
    fs::write(
        &executable,
        "#!/bin/sh\nprintf x >> \"$CODEX_HOME/launched\"\n",
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::start_with_path(home.path(), false, None, Some(bin.path()));
    for route in ["/api/codex/account", "/api/codex/cloud"] {
        let request = format!(
            "GET {route} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 1\r\n",
            service.port
        );
        assert_eq!(service.request_body(&request, "x").status, 400);
        let request = format!(
            "GET {route} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nTransfer-Encoding: chunked\r\n",
            service.port
        );
        assert_eq!(
            service.request_body(&request, "1\r\nx\r\n0\r\n\r\n").status,
            400
        );
    }
    assert!(!home.path().join("launched").exists());
}

#[cfg(unix)]
#[test]
fn cloud_observation_schedules_first_page_and_keeps_it_when_disabled() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    let executable = bin.path().join("codex");
    fs::write(&executable, "#!/bin/sh\nprintf x >> \"$CODEX_HOME/count\"\ncase \"$*\" in *--cursor*) title=next;; *) title=first;; esac\nprintf '{\"tasks\":[{\"id\":\"task_1\",\"title\":\"%s\"}],\"cursor\":\"next\"}\\n' \"$title\"\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::start_with_path(home.path(), false, None, Some(bin.path()));
    let initial = service.get("/api/snapshot?window=all").json();
    assert_eq!(initial["settings"]["observations"]["codex_cloud"], false);
    assert_eq!(initial["settings"]["api_intervals"]["codex_cloud"], 300);
    assert!(!home.path().join("count").exists());
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    assert_eq!(
        post(json!({"observations":{"codex_cloud":true},"api_intervals":{"codex_cloud":59}}))
            .status,
        409
    );
    assert!(!home.path().join("count").exists());
    assert_eq!(
        post(json!({"observations":{"codex_cloud":true},"api_intervals":{"codex_cloud":60}}))
            .status,
        200
    );
    let first = snapshot_when(&service, |value| {
        value["api_queries"]["codex_cloud"]["health"] == "ok"
    });
    assert_eq!(
        first["api_queries"]["codex_cloud"]["items"][0]["title"],
        "first"
    );
    assert_eq!(
        service.get("/api/codex/cloud?cursor=next").json()["items"][0]["title"],
        "next"
    );
    assert_eq!(
        post(json!({"observations":{"codex_cloud":false}})).status,
        200
    );
    let stopped = service.get("/api/snapshot?window=all").json();
    assert_eq!(stopped["settings"]["observations"]["codex_cloud"], false);
    assert_eq!(
        stopped["api_queries"]["codex_cloud"]["items"][0]["title"],
        "first"
    );
    assert_eq!(fs::read_to_string(home.path().join("count")).unwrap(), "xx");
}

#[cfg(unix)]
#[test]
fn cloud_endpoint_validates_cursor_and_caches_each_page() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let bin = tempdir().unwrap();
    let executable = bin.path().join("codex");
    fs::write(&executable, "#!/bin/sh\nprintf x >> \"$CODEX_HOME/count\"\nprintf '%s\\n' '{\"tasks\":[{\"id\":\"task_1\",\"title\":\"Example\",\"secret\":\"PRIVATE\"}],\"cursor\":\"next\"}'\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let service = Service::start_with_path(home.path(), false, None, Some(bin.path()));
    assert!(!home.path().join("count").exists());
    assert_eq!(
        service.get("/api/codex/cloud?cursor=bad%20cursor").status,
        400
    );
    assert_eq!(
        service.get("/api/codex/cloud?cursor=a&cursor=b").status,
        400
    );
    assert!(!home.path().join("count").exists());
    let value = service.get("/api/codex/cloud").json();
    assert_eq!(value["health"], "ok");
    assert!(!value.to_string().contains("PRIVATE"));
    assert_eq!(service.get("/api/codex/cloud").json()["cursor"], "next");
    assert_eq!(
        service.get("/api/codex/cloud?cursor=next").json()["health"],
        "ok"
    );
    assert_eq!(fs::read_to_string(home.path().join("count")).unwrap(), "xx");
}

#[test]
fn mcp_document_preview_detects_structured_text_formats() {
    let home = tempdir().unwrap();
    let root = tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let fixtures = [
        ("settings-json.txt", "{\"enabled\":true}", "JSON"),
        ("settings-lines.txt", "{\"a\":1}\n{\"a\":2}\n", "JSONL"),
        (
            "settings-xml.txt",
            "<root><name>fixture</name></root>",
            "XML",
        ),
        ("settings-table.txt", "name,value\nalpha,1\nbeta,2\n", "CSV"),
        ("settings-object.txt", "{enabled: true}", "Object literal"),
    ];
    let mut args = Vec::new();
    for (name, body, _) in fixtures {
        let path = root.join(name);
        fs::write(&path, body).unwrap();
        args.push(path.to_string_lossy().into_owned());
    }
    let args = serde_json::to_string(&args).unwrap();
    fs::write(
        home.path().join("config.toml"),
        format!("[mcp_servers.fixture]\ncommand = \"node\"\nargs = {args}\n"),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let list = service.get("/api/mcp/files?server=fixture").json();
    let files = list["files"].as_array().unwrap();
    for (name, _, format) in fixtures {
        let id = files.iter().find(|entry| entry["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap();
        let detail = service
            .get(&format!("/api/mcp/files?server=fixture&document={id}"))
            .json();
        assert_eq!(detail["parsed_format"], format, "{name}");
        assert!(detail.get("parsed").is_some());
    }
}

#[test]
fn mcp_document_edit_rejects_yaml_toml_and_origin_failures() {
    let home = tempdir().unwrap();
    let root = tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let yaml = root.join("settings.yaml");
    let toml = root.join("settings.toml");
    fs::write(&yaml, "enabled: false\n").unwrap();
    fs::write(&toml, "enabled = false\n").unwrap();
    fs::write(
        home.path().join("config.toml"),
        format!(
            "[mcp_servers.fixture]\ncommand = \"node\"\nargs = [{},{}]\n",
            serde_json::to_string(yaml.to_str().unwrap()).unwrap(),
            serde_json::to_string(toml.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let list = service.get("/api/mcp/files?server=fixture").json();
    let files = list["files"].as_array().unwrap();
    let post = |value: Value, extra: &str| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/mcp/file HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    for (name, invalid) in [
        ("settings.yaml", "value:\n\tbad: 1\n"),
        ("settings.toml", "enabled = [\n"),
    ] {
        let id = files.iter().find(|entry| entry["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap();
        let detail = service
            .get(&format!("/api/mcp/files?server=fixture&document={id}"))
            .json();
        let sha = detail["sha256"].as_str().unwrap();
        let value = json!({"server":"fixture","document":id,"sha256":sha,"text":invalid});
        assert_eq!(post(value.clone(), "").status, 409);
        assert_eq!(
            post(value, "Origin: http://external.invalid\r\n").status,
            403
        );
    }
    assert_eq!(fs::read_to_string(yaml).unwrap(), "enabled: false\n");
    assert_eq!(fs::read_to_string(toml).unwrap(), "enabled = false\n");
}

#[test]
fn mcp_document_edit_validates_and_preserves_changed_files() {
    let home = tempdir().unwrap();
    let root = tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let file = root.join("settings.json");
    fs::write(&file, "{\"enabled\":false}").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    }
    fs::write(
        home.path().join("config.toml"),
        format!(
            "[mcp_servers.fixture]\ncommand = \"node\"\nargs = [{}]\n",
            serde_json::to_string(file.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let list = service.get("/api/mcp/files?server=fixture").json();
    let id = list["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "settings.json")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let detail = service
        .get(&format!("/api/mcp/files?server=fixture&document={id}"))
        .json();
    assert_eq!(detail["editable"], true);
    assert_eq!(detail["parsed_format"], "JSON");
    let sha = detail["sha256"].as_str().unwrap();
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/mcp/file HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    for text in ["{invalid", r#"{"api_key":"PRIVATE"}"#] {
        assert_eq!(
            post(json!({"server":"fixture","document":id,"sha256":sha,"text":text})).status,
            409
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"enabled\":false}");
    }
    let saved =
        post(json!({"server":"fixture","document":id,"sha256":sha,"text":"{\"enabled\":true}"}));
    assert_eq!(saved.status, 200);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
    assert_eq!(saved.json()["text"], "{\"enabled\":true}");
    assert_eq!(
        post(json!({"server":"fixture","document":id,"sha256":sha,"text":"{}"})).status,
        409
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "{\"enabled\":true}");
    assert!(fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .all(|entry| !entry.file_name().to_string_lossy().starts_with(".lam-")));
    let saved_sha = saved.json()["sha256"].as_str().unwrap().to_owned();
    let config = fs::read_to_string(home.path().join("config.toml")).unwrap();
    fs::write(
        home.path().join("config.toml"),
        "[mcp_servers.fixture]\nenabled = false\n",
    )
    .unwrap();
    assert_eq!(
        post(json!({"server":"fixture","document":id,"sha256":saved_sha,"text":"{}"})).status,
        409
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "{\"enabled\":true}");
    fs::write(home.path().join("config.toml"), config).unwrap();
    let masked = "{\"api_key\":\"PRIVATE_SECRET\"}";
    fs::write(&file, masked).unwrap();
    let masked_detail = service
        .get(&format!("/api/mcp/files?server=fixture&document={id}"))
        .json();
    assert_eq!(masked_detail["editable"], false);
    assert!(!masked_detail.to_string().contains("PRIVATE_SECRET"));
}

#[test]
fn mcp_documents_only_exposes_configured_local_files_and_masks_secrets() {
    let home = tempdir().unwrap();
    let root = tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/server.mjs"), "").unwrap();
    fs::write(root.join("package.json"), "{\"description\":\"Fixture\"}").unwrap();
    fs::write(root.join("README.md"), "Fixture docs").unwrap();
    fs::write(
        root.join(".textlintrc.cjs"),
        "module.exports={rulePaths:['terms.yml']}",
    )
    .unwrap();
    fs::write(root.join("terms.yml"), "version: 1\nrules: []\n").unwrap();
    fs::write(root.join("rules.yaml"), "version: 1\nrules: []\n").unwrap();
    fs::write(
        root.join("settings.json"),
        r#"{"theme":"dark","api_key":"PRIVATE_SECRET"}"#,
    )
    .unwrap();
    fs::write(root.join("credentials.json"), "PRIVATE_CREDENTIAL").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("README.md"), root.join("config-link.md")).unwrap();
        fs::hard_link(root.join("terms.yml"), root.join("rules-hardlink.yml")).unwrap();
    }
    let config = format!(
        "[mcp_servers.fixture]\ncommand = \"node\"\nargs = [{}, {}, {}, {}]\n",
        serde_json::to_string(root.join("src/server.mjs").to_str().unwrap()).unwrap(),
        serde_json::to_string(root.join(".textlintrc.cjs").to_str().unwrap()).unwrap(),
        serde_json::to_string(root.join("settings.json").to_str().unwrap()).unwrap(),
        serde_json::to_string(root.join("credentials.json").to_str().unwrap()).unwrap()
    );
    fs::write(home.path().join("config.toml"), config).unwrap();
    let service = Service::start(home.path(), true, None);
    let list = service.get("/api/mcp/files?server=fixture").json();
    let files = list["files"].as_array().unwrap();
    let names: Vec<_> = files
        .iter()
        .filter_map(|file| file["name"].as_str())
        .collect();
    assert!(names.contains(&"README.md") && names.contains(&"settings.json"));
    assert!(
        !names.contains(&"credentials.json")
            && !names.contains(&"config-link.md")
            && !names.contains(&"rules-hardlink.yml")
    );
    #[cfg(unix)]
    assert!(!names.contains(&"terms.yml"));
    let id = files
        .iter()
        .find(|file| file["name"] == "settings.json")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let detail = service
        .get(&format!("/api/mcp/files?server=fixture&document={id}"))
        .json();
    assert_eq!(detail["masked"], true);
    assert_eq!(detail["editable"], false);
    assert!(!detail.to_string().contains("PRIVATE_SECRET"));
    let rules = files
        .iter()
        .find(|file| file["name"] == "rules.yaml")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(
        service
            .get(&format!("/api/mcp/files?server=fixture&document={rules}"))
            .json()["text"],
        "version: 1\nrules: []\n"
    );
    assert_eq!(
        service
            .get(&format!("/api/mcp/files?server=fixture&document={rules}"))
            .json()["parsed_format"],
        "YAML subset"
    );
    assert_eq!(
        service
            .get("/api/mcp/files?server=fixture&document=00000000000000000000000000000000")
            .status,
        409
    );
    assert_eq!(service.get("/api/mcp/files?server=other").status, 409);
    assert_eq!(
        service
            .get("/api/mcp/files?server=fixture&document=../../secret")
            .status,
        400
    );
}

#[test]
fn worktree_snapshot_reads_loaded_project_and_managed_roots() {
    let home = tempdir().unwrap();
    let home_path = home.path().canonicalize().unwrap();
    let external = tempdir().unwrap();
    let repo = external.path().canonicalize().unwrap().join("repo");
    fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let mut cmd = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        let output = cmd
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .args([
                "-C",
                repo.to_str().unwrap(),
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                if cfg!(windows) {
                    "core.hooksPath=NUL"
                } else {
                    "core.hooksPath=/dev/null"
                },
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "fixture git command failed");
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "fixture"]);
    let managed = home_path.join("worktrees/fixture/linked");
    fs::create_dir_all(managed.parent().unwrap()).unwrap();
    git(&[
        "worktree",
        "add",
        "-q",
        "--detach",
        managed.to_str().unwrap(),
    ]);
    git(&[
        "worktree",
        "lock",
        "--reason",
        "PRIVATE_REASON",
        managed.to_str().unwrap(),
    ]);
    let state = rusqlite::Connection::open(home_path.join("state_1.sqlite")).unwrap();
    state.execute_batch("CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT,cwd TEXT); CREATE TABLE projects(id TEXT,name TEXT,updated_at_ms INTEGER); INSERT INTO projects VALUES('project','Fixture',1); CREATE TABLE project_roots(project_id TEXT,path TEXT)").unwrap();
    state
        .execute(
            "INSERT INTO project_roots VALUES('project',?)",
            [repo.to_str().unwrap()],
        )
        .unwrap();
    state.execute("INSERT INTO threads VALUES('00000000-0000-4000-8000-000000000002','Fixture','project',?)",[repo.to_str().unwrap()]).unwrap();
    let (session, stamp) = native_event_fixture(&home_path);
    append_records(
        &session,
        &[event(
            &stamp,
            "session_meta",
            json!({"id":"00000000-0000-4000-8000-000000000002","cwd":managed.join("src")}),
        )],
    );
    let other = home_path.join("sessions/rollout-00000000-0000-4000-8000-000000000003.jsonl");
    fs::write(
        &other,
        event(
            &stamp,
            "session_meta",
            json!({"id":"00000000-0000-4000-8000-000000000003","cwd":managed.join("nested")}),
        ),
    )
    .unwrap();
    let mut service = Service::start(&home_path, true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["worktrees"]["items"]
            .as_array()
            .is_some_and(|items| items.len() == 2)
    });
    let data = &snapshot["codex"]["worktrees"];
    assert_eq!(data["health"], "ok");
    assert_eq!(data["total"], 2);
    assert_eq!(data["queries"], 1);
    assert!(!data.to_string().contains(repo.to_str().unwrap()));
    assert!(!snapshot.to_string().contains(repo.to_str().unwrap()));
    assert!(!data.to_string().contains("PRIVATE_REASON"));
    assert!(!snapshot
        .to_string()
        .contains(managed.join("nested").to_str().unwrap()));
    let linked = data["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["managed"] == true)
        .unwrap();
    assert_eq!(linked["locked"], true);
    assert_eq!(linked["detached"], true);
    assert_eq!(linked["project_ids"], json!(["project"]));
    assert_eq!(
        linked["thread_ids"],
        json!(["00000000-0000-4000-8000-000000000003"])
    );
    let main = data["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["managed"] == false)
        .unwrap();
    assert_eq!(
        main["thread_ids"],
        json!(["00000000-0000-4000-8000-000000000002"])
    );
    let url = format!("/api/codex/worktree?id={}", linked["id"].as_str().unwrap());
    assert_eq!(service.get(&url).json()["path"], managed.to_str().unwrap());
    git(&["switch", "-q", "-c", "fixture-notified"]);
    snapshot_when(&service, |value| {
        value["codex"]["worktrees"]["items"]
            .as_array()
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item["managed"] == false && item["branch"] == "fixture-notified")
            })
    });
    git(&["worktree", "unlock", managed.to_str().unwrap()]);
    snapshot_when(&service, |value| {
        value["codex"]["worktrees"]["items"]
            .as_array()
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item["managed"] == true && item["locked"] == false)
            })
    });
    state.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let body = "{}";
    assert_eq!(service.request_body(&format!("POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    let partial = snapshot_when(&service, |value| {
        value["codex"]["worktrees"]["root_source_health"] == "partly_unavailable"
    });
    assert!(partial["codex"]["worktrees"]["total"].is_null());
    assert_eq!(partial["codex"]["worktrees"]["observed_total"], 2);
    state.execute_batch("ROLLBACK").unwrap();
    assert_eq!(service.request_body(&format!("POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    snapshot_when(&service, |value| {
        value["codex"]["worktrees"]["root_source_health"] == "ok"
            && value["codex"]["worktrees"]["total"] == 2
    });
    assert_eq!(service.get("/api/codex/worktree?id=bad").status, 400);
    let body = r#"{"observations":{"worktrees":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    assert_eq!(service.get(&url).status, 409);
    assert_eq!(
        service.get("/api/snapshot?window=all").json()["codex"]["worktrees"]["health"],
        "disabled"
    );
    let disabled_path =
        home_path.join("sessions/rollout-00000000-0000-4000-8000-000000000004.jsonl");
    fs::write(
        &disabled_path,
        event(
            &stamp,
            "session_meta",
            json!({"id":"00000000-0000-4000-8000-000000000004","cwd":managed.join("later")}),
        ),
    )
    .unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["threads"].as_array().is_some_and(|threads| {
            threads
                .iter()
                .any(|thread| thread["thread_id"] == "00000000-0000-4000-8000-000000000004")
        })
    });
    let body = r#"{"observations":{"worktrees":true}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    assert_eq!(service.get(&url).json()["path"], managed.to_str().unwrap());
    snapshot_when(&service, |value| {
        value["codex"]["worktrees"]["items"]
            .as_array()
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item["managed"] == true && item["thread_count"] == 2)
            })
    });
    service.stop();
    let db = rusqlite::Connection::open(
        home_path
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT group_concat(payload) FROM session_cursors",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains(managed.to_str().unwrap()));
    let restarted = Service::start(&home_path, true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"]
            .as_u64()
            .is_some_and(|count| count >= 3)
            && value["codex"]["worktrees"]["items"]
                .as_array()
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item["managed"] == true && item["thread_count"] == 2)
                })
    });
    assert_eq!(
        restored["codex"]["worktrees"]["association_scope"],
        "loaded_thread_cwd"
    );
}

#[test]
fn source_registry_describes_loaded_readers_and_disabled_metadata() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let sources = &snapshot["sources"];
    assert_eq!(sources["session"]["health"], "ok");
    assert_eq!(sources["session"]["mode"], "read_only");
    assert!(sources["session"]["locations"][0]
        .as_str()
        .unwrap()
        .ends_with("/sessions"));
    assert_eq!(sources["thread_state"]["health"], "ok");
    assert_eq!(
        sources["activity_history"]["mode"],
        "bounded_metadata_cache"
    );
    assert_eq!(
        sources["activity_history"]["limits"]["retention_days"],
        snapshot["settings"]["activity_retention_days"]
    );
    assert!(snapshot["settings"]["activity_retention_days"].is_number());
    assert!(sources["diagnostics"]["readers"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| row["source"] == "codex_core")));
    assert!(sources["mcp_config"]["locations"][0]
        .as_str()
        .unwrap()
        .ends_with("/config.toml"));
    assert_eq!(sources["mcp_config"]["limits"]["event_limit"], 1000);
    assert_eq!(sources["mcp_config"]["limits"]["tool_limit"], 30);
    assert_eq!(sources["monitor"]["mode"], "bounded_metadata_cache");
    let body = r#"{"observations":{"metadata":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    assert_eq!(
        service.get("/api/snapshot?window=all").json()["sources"]["catalog"]["health"],
        "disabled"
    );
    let disabled_home = tempdir().unwrap();
    let disabled = Service::start(disabled_home.path(), false, None);
    let snapshot = disabled.get("/api/snapshot?window=all").json();
    assert_eq!(snapshot["sources"]["session"]["health"], "disabled");
    assert_eq!(snapshot["sources"]["diagnostics"]["health"], "disabled");
}

#[test]
fn jev_telemetry_error_history_retries_after_storage_failure() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    drop(Store::open(&monitoring, Access::ReadWrite).unwrap());
    let history =
        rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    history.execute_batch("CREATE TRIGGER reject_jev BEFORE INSERT ON history_items WHEN NEW.namespace='errors' BEGIN SELECT RAISE(ABORT,'fixture failure'); END").unwrap();
    let path = home.path().join("jev.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE jev_events(timestamp TEXT,status TEXT,http_attempts INTEGER,input_tokens INTEGER,output_tokens INTEGER,request_bytes INTEGER,response_bytes INTEGER,response_unknown_attempts INTEGER,latency_ms INTEGER,metadata TEXT)").unwrap();
    let time = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let metadata=json!({"timestamp":time,"operation":"rank","status":"failed","attempts":[{"status":"timeout"}]}).to_string();
    db.execute(
        "INSERT INTO jev_events VALUES(?,'failed',1,NULL,NULL,1,0,0,5,?)",
        rusqlite::params![time, metadata],
    )
    .unwrap();
    fs::write(
        monitoring.join("jev-monitor.json"),
        json!({"version":1,"enabled":true,"database":path}).to_string(),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let failed = snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "unavailable"
            && value["errors"]["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["source"] == "jev_telemetry")
            })
    });
    assert_eq!(failed["errors"]["history"]["error_type"], "SQLiteError");
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["total"],
        0
    );
    history.execute_batch("DROP TRIGGER reject_jev").unwrap();
    let body = "{}";
    assert_eq!(service.request_body(&format!("POST /api/refresh HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    snapshot_when(&service, |value| {
        value["errors"]["history"]["health"] == "ok"
            && value["errors"]["history"]["retained"]
                .as_u64()
                .is_some_and(|count| count >= 1)
    });
    assert_eq!(
        service
            .get("/api/history?namespace=errors&section=events")
            .json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["source"] == "jev_telemetry")
            .count(),
        1
    );
}

#[test]
fn jev_telemetry_failures_enter_error_history_and_respect_observation() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    fs::create_dir_all(&monitoring).unwrap();
    let path = home.path().join("jev-errors.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE jev_events(timestamp TEXT,status TEXT,http_attempts INTEGER,input_tokens INTEGER,output_tokens INTEGER,request_bytes INTEGER,response_bytes INTEGER,response_unknown_attempts INTEGER,latency_ms INTEGER,metadata TEXT)").unwrap();
    let time = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let metadata=json!({"timestamp":time,"operation":"rank","status":"failed","prompt":"PRIVATE","attempts":[{"status":"timeout","http_status":504,"body":"SECRET"},{"status":"ok"},{"status":"http_error","http_status":503}]}).to_string();
    db.execute(
        "INSERT INTO jev_events VALUES(?,'failed',3,NULL,NULL,1,0,0,5,?)",
        rusqlite::params![time, metadata],
    )
    .unwrap();
    fs::write(
        monitoring.join("jev-monitor.json"),
        json!({"version":1,"enabled":true,"database":path}).to_string(),
    )
    .unwrap();
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["errors"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .filter(|event| event["source"] == "jev_telemetry")
                .count()
                == 2
        })
    });
    let errors = snapshot["errors"]["events"].as_array().unwrap();
    assert!(errors.iter().any(|event| event["code"] == "timeout"
        && event["attempt"] == 1
        && event["http_status"] == 504));
    assert!(errors
        .iter()
        .any(|event| event["code"] == "http_error" && event["attempt"] == 3));
    assert!(!snapshot["errors"].to_string().contains("PRIVATE"));
    let history = "/api/history?namespace=errors&section=events";
    assert_eq!(
        service.get(history).json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["source"] == "jev_telemetry")
            .count(),
        2
    );
    let body = r#"{"observations":{"errors":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body).status,200);
    assert_eq!(
        service.get("/api/snapshot?window=all").json()["errors"]["total"],
        0
    );
    service.stop();
    let restarted = Service::start(home.path(), false, None);
    let restored = snapshot_when(&restarted, |value| {
        value["errors"]["history"]["health"] == "ok"
            && value["errors"]["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .filter(|event| event["source"] == "jev_telemetry")
                    .count()
                    == 2
            })
    });
    assert!(!restored["errors"].to_string().contains("SECRET"));
}

#[test]
fn jev_cli_configure_only_preserves_existing_database_and_supports_override() {
    let home = tempdir().unwrap();
    let first = home.path().join("first.sqlite");
    let second = home.path().join("second.sqlite");
    let execute = |args: &[&str]| {
        Command::new(BINARY)
            .args([
                "--codex-home",
                home.path().to_str().unwrap(),
                "--configure-only",
            ])
            .args(args)
            .output()
            .unwrap()
    };
    let enabled = execute(&["--enable-jev", "--database", first.to_str().unwrap()]);
    assert!(enabled.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&enabled.stdout).unwrap()["jev_recording"],
        "enabled"
    );
    let path = home.path().join("monitoring/jev-monitor.json");
    let initial: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(initial["database"], first.to_str().unwrap());
    let disabled = execute(&["--disable-jev"]);
    assert!(disabled.status.success());
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["database"], first.to_str().unwrap());
    assert_eq!(saved["enabled_at"], initial["enabled_at"]);
    assert!(!execute(&["--enable-jev", "--disable-jev"]).status.success());
    assert!(!execute(&["--enable-jev", "--database", "relative.sqlite"])
        .status
        .success());
    let replaced = execute(&["--enable-jev", "--database", second.to_str().unwrap()]);
    assert!(replaced.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["database"],
        second.to_str().unwrap()
    );
    assert!(!first.exists() && !second.exists());
}

#[test]
fn jev_recording_batch_keeps_config_when_later_retention_write_fails() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .set_activity_retention(0, std::time::SystemTime::now().into())
        .unwrap();
    drop(store);
    let path = monitoring.join("jev-monitor.json");
    fs::write(&path,json!({"version":1,"enabled":false,"database":home.path().join("fixture.sqlite"),"enabled_at":"2026-11-12T09:00:00.000Z"}).to_string()).unwrap();
    let db = rusqlite::Connection::open(monitoring.join(lam_history_store::FILE_NAME)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_retention BEFORE UPDATE ON history_state WHEN NEW.namespace='activity' BEGIN SELECT RAISE(ABORT,'fixture failure'); END").unwrap();
    let service = Service::start(home.path(), true, None);
    let body = r#"{"recording":true,"activity_retention_days":1,"observations":{"skills":false}}"#;
    let response=service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body);
    assert_eq!(response.status, 503);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["enabled"],
        true
    );
    assert_eq!(
        service.get("/api/settings").json()["observations"]["skills"],
        true
    );
}

#[test]
fn jev_recording_batch_validates_before_applying_other_settings() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    fs::create_dir_all(&monitoring).unwrap();
    let path = monitoring.join("jev-monitor.json");
    let database = home.path().join("fixture.sqlite");
    let before = json!({"version":1,"enabled":false,"database":database,"enabled_at":"2026-11-12T09:00:00.000Z"}).to_string();
    fs::write(&path, &before).unwrap();
    let service = Service::start(home.path(), true, None);
    let post = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),&body)
    };
    assert_eq!(
        post(json!({"recording":true,"observations":{"skills":"invalid"}})).status,
        409
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    fs::write(&path, "invalid").unwrap();
    assert_eq!(
        post(json!({"recording":true,"observations":{"skills":false}})).status,
        409
    );
    assert_eq!(
        service.get("/api/settings").json()["observations"]["skills"],
        true
    );
    fs::write(&path, &before).unwrap();
    assert_eq!(
        post(json!({"recording":true,"observations":{"skills":false}})).status,
        200
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["enabled"],
        true
    );
    let snapshot = snapshot_when(&service, |value| {
        value["jev"]["enabled"] == true && value["settings"]["observations"]["skills"] == false
    });
    assert_eq!(
        snapshot["jev"]["reader"]["locations"][0],
        database.to_string_lossy().as_ref()
    );
}

#[test]
fn jev_recording_endpoint_preserves_database_and_rejects_invalid_config() {
    let home = tempdir().unwrap();
    let monitoring = home.path().join("monitoring");
    fs::create_dir_all(&monitoring).unwrap();
    let path = monitoring.join("jev-monitor.json");
    let database = home.path().join("owned-fixture.sqlite");
    let original = json!({"version":1,"enabled":false,"database":database,"enabled_at":"2026-11-12T09:00:00.000Z"}).to_string();
    fs::write(&path, &original).unwrap();
    let service = Service::start(home.path(), true, None);
    let post = |body: &str| {
        service.request_body(&format!("POST /api/jev/recording HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()),body)
    };
    assert_eq!(post(r#"{"enabled":"true"}"#).status, 400);
    assert_eq!(
        post(r#"{"enabled":true,"database":"/tmp/unapproved"}"#).status,
        400
    );
    let cross_origin = r#"{"enabled":true}"#;
    assert_eq!(service.request_body(&format!("POST /api/jev/recording HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nOrigin: http://external.invalid\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,cross_origin.len()),cross_origin).status,403);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let result = post(r#"{"enabled":true}"#);
    assert_eq!(result.status, 200);
    assert_eq!(result.json()["enabled_at"], "2026-11-12T09:00:00.000Z");
    let updated: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(updated["enabled"], true);
    assert_eq!(updated["database"], database.to_string_lossy().as_ref());
    let backups: Vec<_> = fs::read_dir(&monitoring)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".bak"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), original);
    assert_eq!(post(r#"{"enabled":false}"#).status, 200);
    let updated: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(updated["enabled"], false);
    assert_eq!(updated["database"], database.to_string_lossy().as_ref());
    fs::write(&path, "invalid").unwrap();
    assert_eq!(post(r#"{"enabled":true}"#).status, 409);
    assert_eq!(fs::read_to_string(&path).unwrap(), "invalid");
    assert!(!database.exists());
    assert!(fs::read_dir(&monitoring)
        .unwrap()
        .filter_map(Result::ok)
        .all(|entry| !entry.file_name().to_string_lossy().starts_with(".monitor-")));
}

#[test]
fn jev_watcher_tracks_database_directory_created_later() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let external = tempdir().unwrap();
    let directory = external.path().join("later");
    let database = directory.join("events.sqlite");
    let monitoring = home.path().join("monitoring");
    fs::create_dir_all(&monitoring).unwrap();
    fs::write(
        monitoring.join("jev-monitor.json"),
        json!({"version":1,"enabled":true,"database":database}).to_string(),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["jev"]["health"] == "not_recorded"
            && value["sources"]["telemetry:jev"]["watch_health"] == "ok"
    });
    fs::create_dir(&directory).unwrap();
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch("CREATE TABLE jev_events(timestamp TEXT,status TEXT,http_attempts INTEGER,input_tokens INTEGER,output_tokens INTEGER,request_bytes INTEGER,response_bytes INTEGER,response_unknown_attempts INTEGER,latency_ms INTEGER,metadata TEXT)").unwrap();
    snapshot_when(&service, |value| {
        value["jev"]["health"] == "ok" && value["jev"]["summary"]["calls"] == 0
    });
}

#[test]
fn jev_source_watcher_refreshes_wal_and_reconfigured_database() {
    for codex in [true, false] {
        let home = tempdir().unwrap();
        let external = tempdir().unwrap();
        if codex {
            native_event_fixture(home.path());
        }
        let monitoring = home.path().join("monitoring");
        fs::create_dir_all(&monitoring).unwrap();
        let create = |path: &Path| {
            let db = rusqlite::Connection::open(path).unwrap();
            db.pragma_update(None, "journal_mode", "WAL").unwrap();
            db.execute_batch("CREATE TABLE jev_events(timestamp TEXT,status TEXT,http_attempts INTEGER,input_tokens INTEGER,output_tokens INTEGER,request_bytes INTEGER,response_bytes INTEGER,response_unknown_attempts INTEGER,latency_ms INTEGER,metadata TEXT)").unwrap();
            db
        };
        let first_path = external.path().join("first.sqlite");
        let second_path = external.path().join("second.sqlite");
        let first = create(&first_path);
        let second = create(&second_path);
        let config = monitoring.join("jev-monitor.json");
        fs::write(
            &config,
            json!({"version":1,"enabled":true,"database":first_path}).to_string(),
        )
        .unwrap();
        let service = Service::start(home.path(), codex, None);
        snapshot_when(&service, |value| {
            value["jev"]["health"] == "ok" && value["jev"]["summary"]["calls"] == 0
        });
        let time = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let metadata = json!({"timestamp":time,"status":"ok","operation":"rank"}).to_string();
        first
            .execute(
                "INSERT INTO jev_events VALUES(?,'ok',1,1,1,1,1,0,1,?)",
                rusqlite::params![time, metadata],
            )
            .unwrap();
        snapshot_when(&service, |value| value["jev"]["summary"]["calls"] == 1);
        fs::write(
            &config,
            json!({"version":1,"enabled":true,"database":second_path}).to_string(),
        )
        .unwrap();
        snapshot_when(&service, |value| {
            value["jev"]["health"] == "ok"
                && value["jev"]["summary"]["calls"] == 0
                && value["jev"]["reader"]["locations"][0].as_str() == second_path.to_str()
        });
        second
            .execute(
                "INSERT INTO jev_events VALUES(?,'ok',1,1,1,1,1,0,1,?)",
                rusqlite::params![time, metadata],
            )
            .unwrap();
        snapshot_when(&service, |value| {
            value["jev"]["summary"]["calls"] == 1
                && value["jev"]["reader"]["locations"][0].as_str() == second_path.to_str()
        });
    }
}

#[test]
fn jev_telemetry_snapshot_reads_configured_database_by_window() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let monitoring = home.path().join("monitoring");
    fs::create_dir_all(&monitoring).unwrap();
    let path = home.path().join("jev-events.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE jev_events(timestamp TEXT,status TEXT,http_attempts INTEGER,input_tokens INTEGER,output_tokens INTEGER,request_bytes INTEGER,response_bytes INTEGER,response_unknown_attempts INTEGER,latency_ms INTEGER,metadata TEXT)").unwrap();
    let time = (chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        - chrono::Duration::hours(2))
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let metadata =
        json!({"timestamp":time,"status":"ok","operation":"rank","prompt":"PRIVATE","attempts":[]})
            .to_string();
    db.execute(
        "INSERT INTO jev_events VALUES(?,'ok',1,10,3,20,30,0,5,?)",
        rusqlite::params![time, metadata],
    )
    .unwrap();
    fs::write(
        monitoring.join("jev-monitor.json"),
        json!({"version":1,"enabled":true,"database":path}).to_string(),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let all = snapshot_when(&service, |value| value["jev"]["health"] == "ok");
    assert_eq!(all["jev"]["summary"]["calls"], 1);
    assert_eq!(all["jev"]["recent"][0]["operation"], "rank");
    assert_eq!(all["availability"]["jev"], true);
    assert_eq!(all["mcp"]["telemetry"]["jev"]["summary"]["calls"], 1);
    assert_eq!(all["sources"]["telemetry:jev"]["health"], "ok");
    assert_eq!(
        all["sources"]["telemetry:jev"]["checked_at"],
        all["jev"]["reader"]["checked_at"]
    );
    assert_eq!(
        all["mcp"]["recording_status"]["jev"]["flags"]["config.recording_enabled"]["enabled"],
        true
    );
    assert!(service.get("/api/logs?window=all").json()["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["source"] == "jev_telemetry" && source["health"] == "ok"));
    assert!(!all["jev"].to_string().contains("PRIVATE"));
    assert_eq!(
        service.get("/api/snapshot?window=1h").json()["jev"]["summary"]["calls"],
        0
    );
    let set = |body: &str| {
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",service.port,body.len()), body)
    };
    let disabled = set(r#"{"observations":{"jev":false}}"#);
    assert_eq!(disabled.status, 200);
    assert_eq!(disabled.json()["observations"]["jev"], false);
    let paused = service.get("/api/snapshot?window=all").json();
    assert_eq!(paused["jev"]["health"], "paused");
    assert_eq!(paused["jev"]["enabled"], true);
    assert_eq!(paused["mcp"]["telemetry"]["jev"]["health"], "paused");
    assert_eq!(
        set(r#"{"observations":{"jev":true,"mcp":true}}"#).status,
        200
    );
    assert_eq!(
        service.get("/api/snapshot?window=all").json()["jev"]["summary"]["calls"],
        1
    );
}

#[test]
fn retained_skill_detail_reads_observed_session_tail_after_call_retired() {
    let home = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let root = outside.path().join("retained_skill");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("SKILL.md"), "retained document").unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"retired_skill","arguments":{"cmd":format!("cat '{}'", root.join("SKILL.md").display())}}),
        )],
    );
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["skills"]["counts"]["retained_skill"] == 1
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    service.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    assert_eq!(
        db.execute(
            "DELETE FROM session_calls WHERE call_id='retired_skill'",
            []
        )
        .unwrap(),
        1
    );
    let service = Service::start(home.path(), true, None);
    let restored = snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
            && value["codex"]["skills"]["counts"]["retained_skill"] == 1
    });
    assert_eq!(
        restored["codex"]["skills"]["events"][0]["has_document"],
        false
    );
    let query = "/api/codex/skill?skill=retained_skill&thread_id=00000000-0000-4000-8000-000000000002&call_id=retired_skill";
    assert_eq!(
        service.get(query).json()["documents"][0]["text"],
        "retained document"
    );
    assert_eq!(service.get("/api/codex/skill?skill=retained_skill&thread_id=00000000-0000-4000-8000-000000000002&call_id=unknown").json()["documents"], json!([]));
}

#[test]
fn skill_name_only_detail_rejects_ambiguous_origins() {
    let home = tempdir().unwrap();
    let a = tempdir().unwrap();
    let b = tempdir().unwrap();
    for folder in [a.path(), b.path()] {
        fs::create_dir(folder.join("same")).unwrap();
        fs::write(folder.join("same/SKILL.md"), "content").unwrap();
    }
    let (path, stamp) = native_event_fixture(home.path());
    for (id, folder) in [("first", a.path()), ("second", b.path())] {
        append_records(
            &path,
            &[event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"exec_command","call_id":id,"arguments":{"cmd":format!("cat '{}'", folder.join("same/SKILL.md").display())}}),
            )],
        );
    }
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["skills"]["counts"]["same"] == 2
    });
    assert_eq!(
        service.get("/api/codex/skill?skill=same").json()["documents"],
        json!([])
    );
    assert_eq!(service.get("/api/codex/skill?skill=same&thread_id=00000000-0000-4000-8000-000000000002&call_id=first").json()["documents"].as_array().unwrap().len(), 1);
}

#[test]
fn skill_document_details_require_observed_source_and_filter_sensitive_files() {
    let home = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let root = outside.path().join("alpha");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("references")).unwrap();
    fs::write(root.join("SKILL.md"), "hello password=PRIVATE_SECRET\n").unwrap();
    fs::write(root.join("README.md"), "overview").unwrap();
    fs::write(root.join("references").join("guide.md"), "guide").unwrap();
    fs::write(root.join("credentials.json"), "PRIVATE_CREDENTIAL").unwrap();
    fs::write(root.join("secret.key"), "PRIVATE_KEY").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("README.md"), root.join("linked.md")).unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let command = format!("cat '{}'", root.join("SKILL.md").display());
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"observed_skill","arguments":{"cmd":command}}),
        )],
    );
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["skills"]["counts"]["alpha"]
            .as_u64()
            .is_some_and(|count| count >= 1)
    });
    let id = "00000000-0000-4000-8000-000000000002";
    let url = format!("/api/codex/skill?skill=alpha&thread_id={id}&call_id=observed_skill");
    let detail = service.get(&url).json();
    assert_eq!(detail["documents"].as_array().unwrap().len(), 2);
    assert!(!detail.to_string().contains("PRIVATE_SECRET"));
    let files = detail["files"].as_array().unwrap();
    assert!(files
        .iter()
        .any(|file| file["relative_path"] == "references/guide.md"));
    assert!(!files
        .iter()
        .any(|file| ["credentials.json", "secret.key", "linked.md"]
            .contains(&file["name"].as_str().unwrap_or(""))));
    let selected = service
        .get(&format!("{url}&file=references%2Fguide.md"))
        .json();
    assert_eq!(selected["documents"][0]["text"], "guide");
    assert_eq!(
        service.get(&format!("{url}&file=..%2Fsecret.key")).status,
        400
    );
    let missing = service
        .get(&format!(
            "/api/codex/skill?skill=alpha&thread_id={id}&call_id=unobserved"
        ))
        .json();
    assert_eq!(missing["documents"], json!([]));
    let body = r#"{"observations":{"skills":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), body).status, 200);
    assert_eq!(service.get(&url).status, 409);
}

#[test]
fn skill_snapshot_merges_retained_history_for_selected_threads_and_windows() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"current","arguments":{"cmd":"cat /tmp/shared/SKILL.md"}}),
        )],
    );
    let monitoring = home.path().join("monitoring");
    let mut store = Store::open(&monitoring, Access::ReadWrite)
        .unwrap()
        .unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let old =
        (now - chrono::Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let retained = |thread: &str, call: &str, skill: &str, time: &str| {
        json!({"thread_id":thread,"call_id":call,"skill":skill,"timestamp":time,"has_document":false}).to_string()
    };
    let id = "00000000-0000-4000-8000-000000000002";
    let rows = [
        retained(id, "old", "retained", &old),
        retained(id, "current", "shared", &stamp),
        retained(
            "00000000-0000-4000-8000-000000000099",
            "foreign",
            "excluded",
            &stamp,
        ),
    ];
    let writes: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(index, payload)| lam_history_store::HistoryWrite {
            section: lam_history_store::Section::Skills,
            key: match index {
                0 => "a",
                1 => "b",
                _ => "c",
            },
            ordinal: index as i64,
            event_time: Some(if index == 0 { &old } else { &stamp }),
            payload,
        })
        .collect();
    store
        .save_history(
            lam_history_store::Namespace::Threads,
            r#"{"version":1}"#,
            1,
            None,
            &writes,
        )
        .unwrap();
    let service = Service::start(home.path(), true, None);
    let all = snapshot_when(&service, |value| {
        value["codex"]["skills"]["counts"]["retained"] == 1
            && value["codex"]["skills"]["counts"]["shared"] == 1
    });
    assert_eq!(all["codex"]["skills"]["total"], 2);
    assert!(all["codex"]["skills"]["counts"].get("excluded").is_none());
    let shared = all["codex"]["skills"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["skill"] == "shared")
        .unwrap();
    assert_eq!(shared["has_document"], true);
    let recent = service.get("/api/snapshot?window=1h").json();
    assert!(recent["codex"]["skills"]["counts"]
        .get("retained")
        .is_none());
    assert_eq!(recent["codex"]["skills"]["counts"]["shared"], 1);
}

#[test]
fn skill_history_failure_rolls_back_checkpoint_and_recovers() {
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
    writer.execute_batch("CREATE TRIGGER reject_skill BEFORE INSERT ON history_items WHEN NEW.namespace='threads' AND NEW.section='skills' BEGIN SELECT RAISE(ABORT,'fixture rejected skill'); END").unwrap();
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"skill_write","arguments":{"cmd":"cat /tmp/alpha/SKILL.md"}}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "unavailable"
    });
    let mut reader = Store::open(&monitoring, Access::ReadOnly).unwrap().unwrap();
    assert_eq!(
        reader.checkpoint(&key, 10).unwrap().unwrap().cursor.offset,
        original
    );
    let history_url = "/api/history?namespace=threads&section=skills";
    assert_eq!(service.get(history_url).json()["total"], 0);
    writer.execute_batch("DROP TRIGGER reject_skill").unwrap();
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output","call_id":"skill_write","output":"ok"}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
            && value["codex"]["threads"][0]["tool_events"]
                .as_array()
                .is_some_and(|events| {
                    events.iter().any(|event| {
                        event["call_id"] == "skill_write" && event["completed_at"].is_string()
                    })
                })
    });
    let history = service.get(history_url).json();
    assert_eq!(history["total"], 1);
    assert_eq!(history["items"][0]["skill"], "alpha");
    assert_eq!(history["items"][0]["has_document"], false);
    assert_eq!(
        reader.checkpoint(&key, 10).unwrap().unwrap().cursor.offset,
        fs::metadata(&path).unwrap().len() as i64
    );
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    snapshot_when(&restarted, |value| {
        value["codex"]["read_state"]["checkpoint"]["restored"] == 1
    });
    assert_eq!(restarted.get(history_url).json()["total"], 1);
}

#[test]
fn skill_observation_switch_filters_and_replays_without_disabling_commands() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"initial_skill","arguments":{"cmd":"cat /tmp/alpha/SKILL.md"}}),
        )],
    );
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["codex"]["skills"]["total"] == 1);
    let set = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    let result = set(json!({"observations":{"skills":false}}));
    assert_eq!(result.status, 200);
    assert_eq!(result.json()["observations"]["skills"], false);
    assert_eq!(
        service
            .get("/api/history?namespace=threads&section=skills")
            .status,
        400
    );
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"custom_tool_call","name":"exec","call_id":"disabled_skill","input":"await tools.exec_command({cmd:'cat /tmp/beta/SKILL.md && git status'});"}),
        )],
    );
    let disabled = snapshot_when(&service, |value| {
        value["codex"]["git"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "disabled_skill")
            })
    });
    assert_eq!(disabled["codex"]["skills"]["total"], 0);
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='disabled_skill'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<Value>(&stored)
        .unwrap()
        .get("skills")
        .is_none());
    assert_eq!(
        set(json!({"observations":{"skills":true,"git":true}})).status,
        200
    );
    snapshot_when(&service, |value| {
        value["codex"]["skills"]["counts"]["beta"] == 1
    });
    assert_eq!(
        service
            .get("/api/history?namespace=threads&section=skills")
            .status,
        200
    );
}

#[test]
fn skill_observation_disabled_checkpoint_replays_after_restart() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let body = r#"{"observations":{"skills":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), body).status, 200);
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"replay_skill","arguments":{"cmd":"cat /tmp/gamma/SKILL.md"}}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|bytes| bytes >= fs::metadata(&path).unwrap().len())
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["skills"]["counts"]["gamma"] == 1
    });
    assert!(
        restored["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn skill_read_metadata_projects_commands_without_persisting_paths() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"exec_command","call_id":"direct_skill","arguments":{"cmd":"cat '/PRIVATE_ROOT/alpha/SKILL.md'"}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call","name":"exec","call_id":"nested_skill","input":"await tools.exec_command({cmd:\"sed -n '1,4p' /PRIVATE_ROOT/beta/SKILL.md\"});"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"exec_command","call_id":"not_read","arguments":{"cmd":"echo /PRIVATE_ROOT/ignored/SKILL.md"}}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    let ready = |value: &Value| value["codex"]["skills"]["total"] == 2;
    let snapshot = snapshot_when(&service, ready);
    assert_eq!(snapshot["codex"]["skills"]["counts"]["alpha"], 1);
    assert_eq!(snapshot["codex"]["skills"]["counts"]["beta"], 1);
    assert!(!snapshot["codex"]["skills"]
        .to_string()
        .contains("PRIVATE_ROOT"));
    service.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='direct_skill'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&stored).unwrap()["skills"],
        json!(["alpha"])
    );
    let restarted = Service::start(home.path(), true, None);
    snapshot_when(&restarted, ready);
}

#[test]
fn jev_observation_switch_preserves_mcp_and_replays_disabled_calls() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"mcp__jev__jev_rank","call_id":"initial_jev","arguments":{"query":"hello"}}),
        )],
    );
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["threads"][0]["jev_calls"]
            .as_array()
            .is_some_and(|calls| calls.len() == 1)
    });
    let set = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    let detail = format!("/api/codex/jev?thread={id}&call=initial_jev&index=0");
    let response = set(json!({"observations":{"jev_calls":false}}));
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["observations"]["jev_calls"], false);
    assert_eq!(service.get(&detail).status, 409);
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"custom_tool_call","name":"exec","call_id":"disabled_jev","input":"await tools.mcp__jev__jev_evaluate({text:'sample'});"}),
        )],
    );
    let disabled = snapshot_when(&service, |value| {
        value["mcp"]["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["call_id"] == "disabled_jev")
        })
    });
    assert_eq!(disabled["codex"]["threads"][0]["jev_calls"], json!([]));
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='disabled_jev'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<Value>(&stored)
        .unwrap()
        .get("jev")
        .is_none());
    assert_eq!(
        set(json!({"observations":{"jev_calls":true,"mcp":true}})).status,
        200
    );
    snapshot_when(&service, |value| {
        value["codex"]["threads"][0]["jev_calls"]
            .as_array()
            .is_some_and(|calls| {
                calls.iter().any(|call| {
                    call["call_id"] == "disabled_jev" && call["operation"] == "evaluate"
                })
            })
    });
    assert_eq!(service.get(&detail).status, 200);
}

#[test]
fn jev_observation_disabled_checkpoint_replays_after_restart() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let body = r#"{"observations":{"jev_calls":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), body).status, 200);
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"mcp__jev__jev_status","call_id":"replay_jev","arguments":{}}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|bytes| bytes >= fs::metadata(&path).unwrap().len())
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["threads"][0]["jev_calls"]
            .as_array()
            .is_some_and(|calls| calls.iter().any(|call| call["call_id"] == "replay_jev"))
    });
    assert!(
        restored["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn jev_session_metadata_tracks_direct_and_nested_calls_without_payloads() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    append_records(
        &path,
        &[
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"mcp__jev__jev_rank","call_id":"direct_jev","arguments":{"query":"hello","api_key":"PRIVATE_JEV"}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call_output","call_id":"direct_jev","output":{"value":"ok","password":"PRIVATE_JEV"}}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"custom_tool_call","name":"exec","call_id":"nested_jev","input":"await tools.mcp__jev__jev_evaluate({text:'PRIVATE_JEV'}); await tools.mcp__jev__jev_status({});"}),
            ),
            event(
                &stamp,
                "response_item",
                json!({"type":"function_call","name":"mcp__other__jev_rank","call_id":"other","arguments":{}}),
            ),
        ],
    );
    let mut service = Service::start(home.path(), true, None);
    let assert_events = |value: &Value| {
        value["codex"]["threads"].as_array().is_some_and(|threads| {
            threads.iter().any(|thread| {
                let Some(events) = thread["jev_calls"].as_array() else {
                    return false;
                };
                events.len() == 3
                    && events.iter().any(|e| {
                        e["call_id"] == "direct_jev"
                            && e["operation"] == "rank"
                            && e["nested"] == false
                            && e["index"] == 0
                    })
                    && events.iter().any(|e| {
                        e["call_id"] == "nested_jev"
                            && e["operation"] == "evaluate"
                            && e["nested"] == true
                            && e["index"] == 0
                    })
                    && events.iter().any(|e| {
                        e["call_id"] == "nested_jev"
                            && e["operation"] == "status"
                            && e["nested"] == true
                            && e["index"] == 1
                    })
            })
        })
    };
    let snapshot = snapshot_when(&service, assert_events);
    assert!(!snapshot.to_string().contains("PRIVATE_JEV"));
    let id = "00000000-0000-4000-8000-000000000002";
    let direct = format!("/api/codex/jev?thread={id}&call=direct_jev&index=0");
    let detail = service.get(&direct).json();
    assert_eq!(detail["operation"], "rank");
    assert_eq!(detail["request"]["query"], "hello");
    assert!(!detail.to_string().contains("PRIVATE_JEV"));
    let revealed = service.get(&format!("{direct}&mask=0")).json();
    assert_eq!(revealed["request"]["api_key"], "PRIVATE_JEV");
    assert_eq!(revealed["response"]["password"], "PRIVATE_JEV");
    assert_eq!(service.get(&format!("{direct}&mask=invalid")).status, 400);
    assert_eq!(service.get(&format!("{direct}&mask=0&mask=1")).status, 400);
    assert_eq!(service.get(&direct.replace("index=0", "index=%D9%A0")).status, 200);
    assert_eq!(service.get(&format!("{}&lazy=1", direct.replace("index=0", "index=%2B0"))).status, 400);
    assert_eq!(service.get("/api/codex/jev?thread=bad&call=direct_jev&index=0&lazy=1").status, 400);
    assert_eq!(
        service.get(&format!("{direct}&lazy=1")).json()["request"]["content_page"],
        true
    );
    let revealed_page = service.get(&format!("{direct}&lazy=1&mask=0")).json();
    assert!(revealed_page["request"]["text"].as_str().unwrap().contains("PRIVATE_JEV"));
    let nested = service
        .get(&format!(
            "/api/codex/jev?thread={id}&call=nested_jev&index=1"
        ))
        .json();
    assert_eq!(nested["operation"], "status");
    assert_eq!(nested["response_scope"], "containing_tool_call");
    assert!(nested["response"].is_null());
    assert_eq!(
        service
            .get(&format!("/api/codex/jev?thread={id}&call=other&index=0"))
            .status,
        409
    );
    service.stop();
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT group_concat(payload) FROM session_calls",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains("PRIVATE_JEV"));
    let restarted = Service::start(home.path(), true, None);
    snapshot_when(&restarted, assert_events);
}

#[test]
fn command_observation_switches_filter_independently_and_replay() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"initial_command","arguments":{"cmd":"git status && cargo test"}}),
        )],
    );
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["git"]["total"] == 1
            && value["codex"]["checks"]
                .as_array()
                .is_some_and(|checks| checks.len() == 1)
    });
    let set = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    let git = format!("/api/codex/git?thread_id={id}&call_id=initial_command&operation=status");
    let check =
        format!("/api/codex/check?thread_id={id}&call_id=initial_command&operation=cargo+test");
    let response = set(json!({"observations":{"git":false}}));
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["observations"]["git"], false);
    assert_eq!(service.get(&git).status, 409);
    assert_eq!(service.get(&check).status, 200);
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"custom_tool_call","name":"exec","call_id":"git_disabled","input":"await tools.exec_command({cmd:'git log && cargo check'});"}),
        )],
    );
    let disabled = snapshot_when(&service, |value| {
        value["codex"]["checks"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["call_id"] == "git_disabled")
        })
    });
    assert_eq!(disabled["codex"]["git"]["total"], 0);
    let history = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = history
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='git_disabled'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let stored: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["commands"]["git"], json!([]));
    assert_eq!(stored["commands"]["checks"][0]["operation"], "cargo check");
    assert_eq!(
        set(json!({"observations":{"git":true,"checks":false}})).status,
        200
    );
    let reversed = snapshot_when(&service, |value| {
        value["codex"]["git"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "git_disabled")
            })
    });
    assert_eq!(reversed["codex"]["checks"], json!([]));
    assert_eq!(service.get(&git).status, 200);
    assert_eq!(service.get(&check).status, 409);
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"checks_disabled","arguments":{"cmd":"git diff && cargo build"}}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["git"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "checks_disabled")
            })
    });
    let stored: String = history
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='checks_disabled'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let stored: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(stored["commands"]["git"][0]["operation"], "diff");
    assert_eq!(stored["commands"]["checks"], json!([]));
    assert_eq!(set(json!({"observations":{"checks":true}})).status, 200);
    snapshot_when(&service, |value| {
        value["codex"]["checks"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["call_id"] == "checks_disabled")
        })
    });
    assert_eq!(service.get(&check).status, 200);
}

#[test]
fn command_observation_disabled_checkpoint_replays_after_restart() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let body = r#"{"observations":{"git":false,"checks":false}}"#;
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), body).status, 200);
    append_records(
        &path,
        &[event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"exec_command","call_id":"replay_command","arguments":{"cmd":"git status && cargo test"}}),
        )],
    );
    snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|bytes| bytes >= fs::metadata(&path).unwrap().len())
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["git"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "replay_command")
            })
            && value["codex"]["checks"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "replay_command")
            })
    });
    assert!(
        restored["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn file_result_arriving_while_disabled_replays_after_restart() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__fixture__read_file","call_id":"pending_file","arguments":{"path":"pending.txt"}})).as_bytes()).unwrap();
    drop(file);
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["file_activity"]["total"] == 1
    });
    let body = json!({"observations":{"files":false}}).to_string();
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body).status, 200);
    fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(event(&stamp, "response_item", json!({"type":"function_call_output","call_id":"pending_file","output":{"bytes_read":17}})).as_bytes()).unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["threads"][0]["tool_events"][0]["completed_at"].is_string()
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&restarted, |value| {
        value["codex"]["file_activity"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "pending_file" && event["read_bytes"] == 17)
            })
    });
    assert!(
        snapshot["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn file_result_byte_counts_require_one_observed_invocation() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    let payloads = [
        json!({"type":"function_call","name":"mcp__fixture__read_file","call_id":"direct","arguments":{"path":"direct.txt"}}),
        json!({"type":"function_call_output","call_id":"direct","output":{"structuredContent":{"bytes_read":12,"bytes_written":0,"secret":"PRIVATE_BODY"}}}),
        json!({"type":"custom_tool_call","name":"exec","call_id":"single","input":"await tools.mcp__fixture__read_file({path:'single.txt'});"}),
        json!({"type":"custom_tool_call_output","call_id":"single","output":[{"type":"input_text","text":"{\"status\":\"ok\",\"bytes_read\":23}"}]}),
        json!({"type":"custom_tool_call","name":"exec","call_id":"mixed","input":"await tools.mcp__fixture__read_file({path:'mixed.txt'}); await tools.other({});"}),
        json!({"type":"custom_tool_call_output","call_id":"mixed","output":{"bytes_read":99}}),
    ];
    for payload in payloads {
        file.write_all(event(&stamp, "response_item", payload).as_bytes())
            .unwrap();
    }
    drop(file);
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["file_activity"]["total"] == 3
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let events = snapshot["codex"]["file_activity"]["events"]
        .as_array()
        .unwrap();
    let find = |id| events.iter().find(|event| event["call_id"] == id).unwrap();
    assert_eq!(find("direct")["read_bytes"], 12);
    assert_eq!(find("direct")["write_bytes"], 0);
    assert_eq!(find("single")["read_bytes"], 23);
    assert!(find("mixed").get("read_bytes").is_none());
    assert!(!snapshot.to_string().contains("PRIVATE_BODY"));
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["file_activity"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "direct" && event["read_bytes"] == 12)
            })
    });
    assert!(
        restored["codex"]["read_state"]["checkpoint"]["restored"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn restart_replays_checkpoint_created_while_file_observation_disabled() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    let body = json!({"observations":{"files":false}}).to_string();
    assert_eq!(service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body).status, 200);
    fs::OpenOptions::new().append(true).open(&session).unwrap().write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__fixture__read_file","call_id":"restart_file","arguments":{"path":"replayed.txt"}})).as_bytes()).unwrap();
    snapshot_when(&service, |value| {
        value["codex"]["observed_tool_calls"] == 1
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&restarted, |value| {
        value["codex"]["file_activity"]["total"] == 1
    });
    assert_eq!(
        snapshot["codex"]["file_activity"]["events"][0]["path"],
        "replayed.txt"
    );
    assert!(
        snapshot["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn observed_file_metadata_rejects_unobserved_and_protected_paths() {
    let home = tempdir().unwrap();
    let (session, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let physical_home = home.path().canonicalize().unwrap();
    let file_path = physical_home.join("document.txt");
    let secret_path = physical_home.join(".env");
    fs::write(&file_path, "fixture").unwrap();
    fs::write(&secret_path, "private").unwrap();
    let mut output = fs::OpenOptions::new().append(true).open(&session).unwrap();
    for (call, path) in [("file1", &file_path), ("secret", &secret_path)] {
        output.write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__fixture__read_file","call_id":call,"arguments":{"path":path}})).as_bytes()).unwrap();
    }
    #[cfg(unix)]
    {
        let link = physical_home.join("link.txt");
        std::os::unix::fs::symlink(&file_path, &link).unwrap();
        output.write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__fixture__read_file","call_id":"link","arguments":{"path":link}})).as_bytes()).unwrap();
    }
    drop(output);
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| {
        value["codex"]["file_activity"]["total"]
            .as_u64()
            .is_some_and(|total| total >= 2)
    });
    let detail = |call: &str, path: &Path| {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("thread_id", id)
            .append_pair("call_id", call)
            .append_pair("path", path.to_str().unwrap())
            .finish();
        service.get(&format!("/api/codex/file?{query}"))
    };
    let result = detail("file1", &file_path).json();
    assert_eq!(result["health"], "ok");
    assert_eq!(result["bytes"], 7);
    assert_eq!(detail("secret", &secret_path).json()["health"], "rejected");
    assert_eq!(detail("file1", &secret_path).status, 409);
    #[cfg(unix)]
    assert_eq!(
        detail("link", &physical_home.join("link.txt")).json()["health"],
        "rejected"
    );
    let summary = service.get("/api/codex/file-summary?window=all").json();
    assert!(summary["total"].as_u64().unwrap() >= 2);
    assert!(summary["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["path"] == file_path.to_str().unwrap() && row["bytes"] == 7));
    assert_eq!(
        service
            .get("/api/codex/file-summary?window=all&window=1h")
            .status,
        400
    );
    let set_files = |enabled: bool| {
        let body = json!({"observations":{"files":enabled}}).to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    assert_eq!(set_files(false).status, 200);
    let previous_calls = service.get("/api/snapshot?window=all").json()["codex"]
        ["observed_tool_calls"]
        .as_u64()
        .unwrap();
    assert_eq!(detail("file1", &file_path).status, 409);
    assert_eq!(
        service.get("/api/codex/file-summary?window=all").status,
        409
    );
    let hidden_path = physical_home.join("disabled-marker.txt");
    fs::write(&hidden_path, "fixture").unwrap();
    fs::OpenOptions::new().append(true).open(&session).unwrap().write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"mcp__fixture__read_file","call_id":"disabled_call","arguments":{"path":hidden_path}})).as_bytes()).unwrap();
    let disabled = snapshot_when(&service, |value| {
        value["codex"]["observed_tool_calls"]
            .as_u64()
            .is_some_and(|count| count > previous_calls)
    });
    assert_eq!(disabled["codex"]["file_activity"]["total"], 0);
    assert_eq!(disabled["codex"]["threads"][0]["file_reads"], json!([]));
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='disabled_call'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains("disabled-marker.txt"));
    drop(db);
    assert_eq!(set_files(true).status, 200);
    snapshot_when(&service, |value| {
        value["codex"]["file_activity"]["events"]
            .as_array()
            .is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["call_id"] == "disabled_call")
            })
    });
    assert_eq!(detail("disabled_call", &hidden_path).status, 200);
}

#[test]
fn file_activity_projects_patch_and_nested_tool_without_body() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(event(&stamp, "response_item", json!({"type":"custom_tool_call","name":"apply_patch","call_id":"patch","input":"*** Begin Patch\n*** Update File: src/a.rs\n+PRIVATE_CONTENT\n*** End Patch"})).as_bytes()).unwrap();
    file.write_all(event(&stamp, "response_item", json!({"type":"custom_tool_call","name":"exec","call_id":"nested","input":"await tools.mcp__fixture__read_file({path:'docs/b.txt'});"})).as_bytes()).unwrap();
    file.write_all(event(&stamp, "response_item", json!({"type":"function_call","name":"exec_command","call_id":"shell","arguments":{"cmd":"head -n 5 docs/c.txt","workdir":"/tmp"}})).as_bytes()).unwrap();
    drop(file);
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["codex"]["file_activity"]["total"] == 3
            && value["codex"]["read_state"]["checkpoint"]["write_health"] == "ok"
    });
    assert_eq!(
        snapshot["codex"]["file_activity"]["operations"]["modified"],
        1
    );
    assert_eq!(snapshot["codex"]["file_activity"]["operations"]["read"], 2);
    assert_eq!(
        snapshot["codex"]["threads"][0]["file_changes"][0]["path"],
        "src/a.rs"
    );
    let reads = snapshot["codex"]["threads"][0]["file_reads"]
        .as_array()
        .unwrap();
    assert!(reads.iter().any(|row| row["path"] == "docs/b.txt"));
    assert!(reads
        .iter()
        .any(|row| row["path"] == "docs/c.txt" && row["range"]["first_lines"] == 5));
    assert!(!snapshot.to_string().contains("PRIVATE_CONTENT"));
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["file_activity"]["total"] == 3
    });
    assert!(
        restored["codex"]["read_state"]["checkpoint"]["restored"]
            .as_u64()
            .unwrap()
            >= 1
    );
}

#[test]
fn monitor_samples_collection_cpu_with_debug_disabled() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    let call = event(
        &stamp,
        "response_item",
        json!({"type":"function_call","name":"fixture","call_id":"call1","arguments":"{}"}),
    );
    file.write_all(call.as_bytes()).unwrap();
    file.write_all(b"unfinished").unwrap();
    drop(file);
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["monitor"]["history"]
            .as_array()
            .is_some_and(|history| !history.is_empty())
    });
    let monitor = &snapshot["monitor"];
    assert_eq!(monitor["debug"]["enabled"], false);
    assert_eq!(monitor["health"], "ok");
    assert!(monitor["pid"].as_u64().is_some_and(|pid| pid > 0));
    assert!(monitor["refreshes"]
        .as_u64()
        .is_some_and(|count| count >= 1));
    assert!(monitor["uptime_seconds"].as_u64().is_some());
    assert_eq!(monitor["history_limit"], 360);
    let sample = monitor["history"].as_array().unwrap().last().unwrap();
    assert!(sample["time"].as_str().is_some());
    assert!(sample["refresh_ms"]
        .as_f64()
        .is_some_and(|value| value >= 0.0));
    assert!(sample["cpu_ms"].as_f64().is_some_and(|value| value >= 0.0));
    assert_eq!(monitor["cpu_ms"], sample["cpu_ms"]);
    assert!(sample["phase_metrics"]["sessions"]["elapsed_ms"]
        .as_f64()
        .is_some());
    assert!(sample["phase_metrics"]["history"]["cpu_ms"]
        .as_f64()
        .is_some());
    assert!(monitor["max_phase_cpu_ms"]
        .as_f64()
        .is_some_and(|value| value >= 0.0));
    assert_eq!(sample["retained_calls"], 1);
    assert_eq!(sample["buffer_bytes"], 10);
    assert_eq!(sample["call_limit"], 50000);
    assert_eq!(sample["file_limit"], 5000);
    assert_eq!(sample["buffer_limit"], 8 * 1024 * 1024);
}

#[test]
fn idle_snapshot_and_new_stream_report_current_uptime() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let before = snapshot_when(&service, |value| value["monitor"]["device_ready"] == true);
    std::thread::sleep(Duration::from_millis(1200));
    let after = service.get("/api/snapshot?window=all").json();
    assert!(
        after["monitor"]["uptime_seconds"].as_u64().unwrap()
            > before["monitor"]["uptime_seconds"].as_u64().unwrap()
    );
    assert_eq!(
        after["monitor"]["refreshes"],
        before["monitor"]["refreshes"]
    );
    let mut stream = EventStream::connect(&service, "?window=all").unwrap();
    let frame = stream.snapshot();
    let streamed = frame["snapshot"]["monitor"]["uptime_seconds"]
        .as_u64()
        .unwrap();
    assert!(streamed >= after["monitor"]["uptime_seconds"].as_u64().unwrap());
}

#[test]
fn monitor_collection_sample_counts_desktop_log_bytes() {
    let home = tempdir().unwrap();
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let line = "2026-11-12T00:00:00Z INFO desktop fixture\n";
    fs::write(directory.join("codex-desktop-fixture.log"), line).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| {
        value["monitor"]["history"]
            .as_array()
            .is_some_and(|samples| {
                samples.iter().any(|sample| {
                    sample["read_bytes"]
                        .as_u64()
                        .is_some_and(|bytes| bytes >= line.len() as u64)
                })
            })
    });
    assert_eq!(snapshot["codex"]["bytes_read"], 0);
}

#[test]
fn tool_detail_reads_response_id_without_marking_call_completed() {
    let home = tempdir().unwrap();
    let (path, stamp) = native_event_fixture(home.path());
    let id = "00000000-0000-4000-8000-000000000002";
    let rows = [
        event(&stamp, "session_meta", json!({"id":id})),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call","name":"fixture_tool","call_id":"","id":"call1","arguments":"{}"}),
        ),
        event(
            &stamp,
            "response_item",
            json!({"type":"function_call_output","id":"call1","output":"result"}),
        ),
    ];
    fs::write(&path, rows.concat()).unwrap();
    let mut service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 1);
    assert!(snapshot["codex"]["threads"][0]["tool_events"][0]["completed_at"].is_null());
    let detail = service
        .get(&format!("/api/codex/tool?thread_id={id}&call_id=call1"))
        .json();
    assert_eq!(detail["response"], "result");
    let set_tool_events = |enabled: bool| {
        let body = json!({"observations":{"tool_events":enabled}}).to_string();
        service.request_body(
            &format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()),
            &body,
        )
    };
    assert_eq!(set_tool_events(false).status, 200);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["settings"]["observations"]["tool_events"], false);
    assert_eq!(disabled["codex"]["threads"][0]["tool_events"], json!([]));
    assert_eq!(disabled["codex"]["observed_tool_calls"], 1);
    let appended = event(
        &stamp,
        "response_item",
        json!({"type":"custom_tool_call","name":"exec","call_id":"call2","input":"await tools.private_marker({}); await tools.mcp__future__get({});"}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(appended.as_bytes())
        .unwrap();
    let disabled = snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 2);
    let db = rusqlite::Connection::open(
        home.path()
            .join("monitoring")
            .join(lam_history_store::FILE_NAME),
    )
    .unwrap();
    let stored: String = db
        .query_row(
            "SELECT payload FROM session_calls WHERE call_id='call2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains("private_marker"));
    assert!(stored.contains("mcp__future__get"));
    drop(db);
    let read_before_reenable = disabled["codex"]["bytes_read"].as_u64().unwrap();
    assert_eq!(
        service
            .get(&format!("/api/codex/tool?thread_id={id}&call_id=call1"))
            .status,
        409
    );
    assert_eq!(set_tool_events(true).status, 200);
    let rebuilt = snapshot_when(&service, |value| {
        value["codex"]["bytes_read"]
            .as_u64()
            .is_some_and(|count| count > read_before_reenable)
            && value["codex"]["observed_tool_calls"] == 2
    });
    assert_eq!(rebuilt["codex"]["nested_tools"]["private_marker"], 1);
    assert_eq!(
        service
            .get(&format!("/api/codex/tool?thread_id={id}&call_id=call1"))
            .status,
        200
    );
    assert_eq!(set_tool_events(false).status, 200);
    let appended = event(
        &stamp,
        "response_item",
        json!({"type":"custom_tool_call","name":"exec","call_id":"call3","input":"await tools.restart_marker({});"}),
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(appended.as_bytes())
        .unwrap();
    snapshot_when(&service, |value| value["codex"]["observed_tool_calls"] == 3);
    service.stop();
    let restarted = Service::start(home.path(), true, None);
    let restored = snapshot_when(&restarted, |value| {
        value["codex"]["observed_tool_calls"] == 3
    });
    assert_eq!(restored["codex"]["nested_tools"]["restart_marker"], 1);
    assert!(
        restored["codex"]["read_state"]["checkpoint"]["invalidated"]
            .as_u64()
            .unwrap()
            >= 1
    );
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
fn missing_codex_home_is_created_only_after_startup_checks() {
    let profile = tempdir().unwrap();
    let rejected_home = profile.path().join("rejected");
    let rejected = Command::new(BINARY)
        .arg("--codex-home").arg(&rejected_home)
        .args(["--port", "0", "--assets"]).arg(profile.path().join("missing-assets"))
        .output().unwrap();
    assert!(!rejected.status.success());
    assert!(!rejected_home.exists());

    let home = profile.path().join("missing").join("..").join("selected");
    let started = Command::new(BINARY)
        .arg("--codex-home").arg(&home)
        .args(["--port", "0", "--watch-stdin"])
        .stdin(Stdio::null()).output().unwrap();
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));
    let first_line = String::from_utf8_lossy(&started.stdout).lines().next().unwrap().to_owned();
    assert_eq!(serde_json::from_str::<Value>(&first_line).unwrap()["status"], "listening");
    assert!(profile.path().join("selected/monitoring").is_dir());
    assert!(!profile.path().join("missing").exists());
}

#[cfg(unix)]
#[test]
fn codex_home_expands_tilde_before_startup() {
    let profile = tempdir().unwrap();
    let selected = profile.path().join("selected");
    fs::create_dir(&selected).unwrap();
    let result = Command::new(BINARY)
        .args(["--codex-home", "~/selected", "--port", "0", "--watch-stdin"])
        .env("HOME", profile.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let first_line = String::from_utf8_lossy(&result.stdout).lines().next().unwrap().to_owned();
    assert_eq!(serde_json::from_str::<Value>(&first_line).unwrap()["status"], "listening");
    assert!(selected.join("monitoring").is_dir());
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
                (
                    "workbench-ui.js",
                    "global.WorkbenchUI = Object.freeze({ version: \"0.13.0\" });",
                ),
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
    assert_eq!(
        service.get("/workbench-ui.js").body,
        b"global.WorkbenchUI = Object.freeze({ version: \"0.13.0\" });"
    );
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

#[test]
fn rejects_old_workbench_version_even_with_matching_manifest() {
    let home = tempdir().unwrap();
    let assets = tempdir().unwrap();
    bundle(assets.path());
    let directory = assets.path().join("_workbench");
    let source = "global.WorkbenchUI = Object.freeze({ version: \"0.12.9\" });";
    fs::write(directory.join("workbench-ui.js"), source).unwrap();
    let manifest_path = directory.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["sha256"]["workbench-ui.js"] =
        json!(format!("{:x}", Sha256::digest(source.as_bytes())));
    fs::write(manifest_path, manifest.to_string()).unwrap();
    let result = Command::new(BINARY)
        .args(["--port", "0", "--codex-home"])
        .arg(home.path())
        .arg("--assets")
        .arg(assets.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Workbench UI 0.13.0 or later"));
}

#[test]
fn selected_workbench_development_source_is_served() {
    let home = tempdir().unwrap();
    let project = tempdir().unwrap();
    let source = tempdir().unwrap();
    let package = project.path().join("src/local_activity_monitor");
    fs::create_dir_all(&package).unwrap();
    bundle(&package);
    fs::write(
        project.path().join("workbench-ui.json"),
        include_str!("../../../workbench-ui.json"),
    )
    .unwrap();
    fs::create_dir(project.path().join(".local")).unwrap();
    let pin: Value = serde_json::from_str(include_str!("../../../workbench-ui.json")).unwrap();
    fs::write(project.path().join(".local/workbench-ui-development.json"), json!({"version":1,"source":source.path().canonicalize().unwrap(),"revision":pin["revision"]}).to_string()).unwrap();
    fs::create_dir_all(source.path().join("integrations/python")).unwrap();
    fs::write(
        source
            .path()
            .join("integrations/python/workbench_assets.py"),
        "",
    )
    .unwrap();
    fs::create_dir(source.path().join("src")).unwrap();
    let development = "global.WorkbenchUI = Object.freeze({ version: \"0.23.0\" }); // development";
    for (name, data) in [
        ("workbench-ui.js", development),
        ("workbench-ui.css", ".dev{}"),
        ("workbench-ui.mjs", ""),
        ("workbench-loader.js", ""),
    ] {
        fs::write(source.path().join("src").join(name), data).unwrap();
    }
    let mut child = Command::new(BINARY)
        .args(["--port", "0", "--watch-stdin", "--codex-home"])
        .arg(home.path())
        .arg("--assets")
        .arg(&package)
        .env_remove("WORKBENCH_UI_PATH")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let ready: Value = serde_json::from_str(&line).unwrap();
    let port = ready["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://127.0.0.1:")
        .unwrap()
        .trim_end_matches('/')
        .parse()
        .unwrap();
    let mut service = Service { child, port };
    assert_eq!(service.get("/workbench-ui.js").body, development.as_bytes());
    service.stop();
    let override_source = tempdir().unwrap();
    fs::create_dir_all(override_source.path().join("integrations/python")).unwrap();
    fs::write(
        override_source
            .path()
            .join("integrations/python/workbench_assets.py"),
        "",
    )
    .unwrap();
    fs::create_dir(override_source.path().join("src")).unwrap();
    let overridden = "global.WorkbenchUI = Object.freeze({ version: \"0.23.0\" }); // override";
    for (name, data) in [
        ("workbench-ui.js", overridden),
        ("workbench-ui.css", ".override{}"),
        ("workbench-ui.mjs", ""),
        ("workbench-loader.js", ""),
    ] {
        fs::write(override_source.path().join("src").join(name), data).unwrap();
    }
    let mut child = Command::new(BINARY)
        .args(["--port", "0", "--watch-stdin", "--codex-home"])
        .arg(home.path())
        .arg("--assets")
        .arg(&package)
        .env("WORKBENCH_UI_PATH", override_source.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let ready: Value = serde_json::from_str(&line).unwrap();
    let port = ready["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://127.0.0.1:")
        .unwrap()
        .trim_end_matches('/')
        .parse()
        .unwrap();
    let mut service = Service { child, port };
    assert_eq!(service.get("/workbench-ui.js").body, overridden.as_bytes());
    service.stop();
    let selection = project.path().join(".local/workbench-ui-development.json");
    let invalid = json!({"version":1,"source":source.path().canonicalize().unwrap(),"revision":"a".repeat(40)}).to_string();
    fs::write(&selection, &invalid).unwrap();
    let failed = Command::new(BINARY)
        .args(["--port", "0", "--codex-home"])
        .arg(home.path())
        .arg("--assets")
        .arg(&package)
        .env("WORKBENCH_UI_PATH", override_source.path())
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr)
        .contains("Invalid Workbench UI development selection"));
    assert_eq!(fs::read_to_string(selection).unwrap(), invalid);
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
    let snapshot = snapshot_when(&service, |snapshot| {
        snapshot["codex"]["schedules"]["remote_activities"][0]["name"] == "First"
            && snapshot["codex"]["metadata_sources"]
                .as_array()
                .is_some_and(|sources| {
                    sources.iter().any(|source| {
                        source["name"] == "session_index.jsonl" && source["matched_threads"] == 1
                    })
                })
    });
    let report = snapshot["codex"]["metadata_sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["name"] == "session_index.jsonl")
        .unwrap();
    assert_eq!(report["health"], "ok");
    assert_eq!(report["tail_bytes"], 1_048_576);
    assert!(snapshot["sources"]["catalog"]["readers"]
        .as_array()
        .unwrap()
        .contains(report));
    let catalog = snapshot["codex"]["metadata_sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["name"] == "codex-dev.db")
        .unwrap();
    assert_eq!(catalog["health"], "ok");
    assert_eq!(catalog["rows_read"], 1);
    assert_eq!(catalog["queries"][0]["health"], "unsupported");
    assert_eq!(catalog["queries"][1]["health"], "unsupported");
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
fn overflow_tool_detail_uses_verified_source_and_masks_secrets() {
    let home = tempdir().unwrap();
    let root = home.path().join("sessions");
    fs::create_dir(&root).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let path = root.join(format!("rollout-{id}.jsonl"));
    let time = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let body = [event(&time,"session_meta",json!({"id":id,"cwd":"/fixture/private-workdir"})),event(&time,"response_item",json!({"type":"function_call","name":"fixture_tool","call_id":"outside","arguments":"{\"api_key\":\"PRIVATE_VALUE\",\"message\":\"hello\"}"}))].concat();
    fs::write(&path, body).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_600_000_000)),
        )
        .unwrap();
    for index in 2..=5001 {
        let other = format!("00000000-0000-4000-8000-{index:012}");
        fs::write(root.join(format!("rollout-{other}.jsonl")), "").unwrap();
    }
    let service = Service::start(home.path(), true, None);
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let snapshot = service.get("/api/snapshot?window=all").json();
        let ready = snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["thread_id"] == id
                    && row["tool_calls"] == 1
                    && row["source_details_pending"].is_null()
            })
        });
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "overflow detail did not become ready"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let endpoint = format!("/api/codex/tool?thread_id={id}&call_id=outside");
    let response = service.get(&endpoint);
    assert_eq!(response.status, 200);
    let detail = response.json();
    assert_eq!(detail["tool"], "fixture_tool");
    assert!(!detail.to_string().contains("PRIVATE_VALUE"));
    let setting = r#"{"observations":{"metadata":false}}"#;
    let response = service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, setting.len()), setting);
    assert!(matches!(response.status, 200 | 202));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let snapshot = service.get("/api/snapshot?window=all").json();
        let ready = snapshot["settings"]["observations"]["metadata"] == false
            && snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
                rows.iter()
                    .any(|row| row["thread_id"] == id && row["source_details_pending"].is_null())
            });
        assert!(!snapshot.to_string().contains("/fixture/private-workdir"));
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "overflow workdir was not retained after metadata disabled"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(service.get(&endpoint).status, 200);
    let key = checkpoint_key(Path::new(&format!("rollout-{id}.jsonl")));
    let mut store = Store::open(&home.path().join("monitoring"), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let mut cursor = store.checkpoint(&key, 0).unwrap().unwrap().cursor;
    cursor.signature = "changed-by-test".into();
    store.save_checkpoints(&[cursor], &[], &[]).unwrap();
    assert_eq!(service.get(&endpoint).status, 409);
}

#[test]
fn session_discovery_reads_nested_directories_beyond_eight_levels() {
    let home = tempdir().unwrap();
    let mut root = home.path().join("sessions");
    for _ in 0..12 {
        root.push("nested");
    }
    fs::create_dir_all(&root).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(
        root.join(format!("rollout-{id}.jsonl")),
        [
            event(&stamp, "session_meta", json!({"id":id})),
            event(&stamp, "event_msg", json!({"type":"task_started"})),
        ]
        .concat(),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |snapshot| {
        snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["thread_id"] == id && row["status"] == "running")
        })
    });
    assert_eq!(snapshot["codex"]["health"], "ok");
}

#[test]
fn python_lifecycle_cache_restores_matching_session_status() {
    let home = tempdir().unwrap();
    let root = home.path().join("sessions");
    fs::create_dir(&root).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let filename = format!("rollout-{id}.jsonl");
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let earlier = (chrono::DateTime::parse_from_rfc3339(&stamp).unwrap()
        - chrono::Duration::seconds(1))
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let body = [
        event(&earlier, "session_meta", json!({"id":id})),
        event(&earlier, "event_msg", json!({"type":"task_started"})),
    ]
    .concat();
    fs::write(root.join(&filename), body.as_bytes()).unwrap();
    let other_id = "00000000-0000-4000-8000-000000000002";
    let other_filename = format!("rollout-{other_id}.jsonl");
    let other_body = event(&stamp, "session_meta", json!({"id":other_id}));
    fs::write(root.join(&other_filename), other_body.as_bytes()).unwrap();
    let monitoring = home.path().join("monitoring");
    fs::create_dir(&monitoring).unwrap();
    fs::write(monitoring.join("thread-state.json"), json!({"version":1,"entries":{filename.clone():{"thread_id":id,"offset":body.len(),"status":"completed","task_time":stamp,"task_start":null},other_filename:{"thread_id":other_id,"offset":other_body.len()+1,"status":"completed","task_time":stamp,"task_start":null}},"skills":[]}).to_string()).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |snapshot| {
        snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["thread_id"] == id && row["status_source"] == "cached_lifecycle")
        })
    });
    let row = snapshot["codex"]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["thread_id"] == id)
        .unwrap();
    assert_eq!(row["status"], "completed");
    assert_eq!(row["status_updated_at"], stamp);
    let other = snapshot["codex"]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["thread_id"] == other_id)
        .unwrap();
    assert_eq!(other["status"], "observed");
    assert!(other["status_source"].is_null());
    let later = (chrono::DateTime::parse_from_rfc3339(&stamp).unwrap()
        + chrono::Duration::seconds(1))
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::OpenOptions::new()
        .append(true)
        .open(root.join(&filename))
        .unwrap()
        .write_all(event(&later, "event_msg", json!({"type":"task_started"})).as_bytes())
        .unwrap();
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["thread_id"] == id
                    && row["status"] == "running"
                    && row["status_source"] == "session_lifecycle"
            })
        })
    });
}

#[test]
fn session_lifecycle_writes_python_compatible_history() {
    let home = tempdir().unwrap();
    let root = home.path().join("sessions");
    fs::create_dir(&root).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let filename = format!("rollout-{id}.jsonl");
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let body = [
        event(&stamp, "session_meta", json!({"id":id})),
        event(&stamp, "event_msg", json!({"type":"task_complete"})),
    ]
    .concat();
    fs::write(root.join(&filename), body.as_bytes()).unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["thread_id"] == id && row["status"] == "completed")
        })
    });
    let store = Store::open(&home.path().join("monitoring"), Access::ReadOnly)
        .unwrap()
        .unwrap();
    let cached = store.thread_lifecycle(&filename).unwrap().unwrap();
    assert_eq!(cached["thread_id"], id);
    assert_eq!(cached["status"], "completed");
    assert_eq!(cached["offset"], body.len());
    assert_eq!(cached["task_time"], stamp);
}

#[test]
fn related_subagent_tail_is_projected_into_snapshot() {
    let home = tempdir().unwrap();
    let root = home.path().join("sessions");
    fs::create_dir(&root).unwrap();
    let parent = "00000000-0000-4000-8000-000000000001";
    let child = "00000000-0000-4000-8000-000000000002";
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let started = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let completed = (now + chrono::Duration::milliseconds(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(
        root.join(format!("rollout-{child}-suffix.jsonl")),
        [
            event(&started, "session_meta", json!({"id":parent})),
            event(&completed, "event_msg", json!({"type":"task_complete"})),
        ]
        .concat(),
    )
    .unwrap();
    let db = rusqlite::Connection::open(home.path().join("state_1.sqlite")).unwrap();
    db.execute_batch(&format!("CREATE TABLE threads(id TEXT,title TEXT); CREATE TABLE thread_spawn_edges(parent_thread_id TEXT,child_thread_id TEXT); INSERT INTO threads VALUES('{child}','Child'); INSERT INTO thread_spawn_edges VALUES('{parent}','{child}');")).unwrap();
    let service = Service::start(home.path(), true, None);
    let snapshot = snapshot_when(&service, |snapshot| {
        snapshot["codex"]["threads"].as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["thread_id"] == child && row["status_source"] == "session_lifecycle")
        })
    });
    let child_row = snapshot["codex"]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["thread_id"] == child)
        .unwrap();
    assert_eq!(child_row["status"], "completed");
    assert_eq!(child_row["status_updated_at"], completed);
    let report = snapshot["codex"]["metadata_sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["name"] == "subagent_lifecycle")
        .unwrap();
    assert_eq!(report["health"], "ok");
    assert_eq!(report["pending_files"], 0);
}

#[test]
fn disabled_model_api_reports_both_sources_in_original_order() {
    let home = tempdir().unwrap();
    let service = Service::start(home.path(), false, None);
    let snapshot = service.get("/api/snapshot?window=all").json();
    let sources = snapshot["model_api"]["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0]["source"], "codex_desktop");
    assert_eq!(sources[1]["source"], "codex_core");
    assert!(sources.iter().all(|source| source["health"] == "disabled"));
}

#[test]
fn future_sql_activity_is_published_after_due_without_source_change() {
    let home = tempdir().unwrap();
    let root = home.path().join("sessions");
    fs::create_dir(&root).unwrap();
    let id = "00000000-0000-4000-8000-000000000001";
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let stamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let future =
        (now + chrono::Duration::seconds(62)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    fs::write(root.join(format!("rollout-demo-{id}.jsonl")), [event(&stamp,"session_meta",json!({"id":id})), event(&future,"response_item",json!({"type":"function_call","name":"mcp__sqlite__query","call_id":"future","arguments":{"query":"SELECT 1"}}))].concat()).unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["observed_tool_calls"] == 1
    });
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if service.get("/api/history?section=sql").json()["total"] == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "future activity was not published"
        );
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[test]
fn future_history_is_promoted_when_codex_collection_is_disabled() {
    let home = tempdir().unwrap();
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let future =
        (now + chrono::Duration::seconds(62)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut store = lam_history_store::ObservationStore::open(
        &home.path().join("monitoring"),
        Access::ReadWrite,
    )
    .unwrap()
    .unwrap();
    store.save_diagnostics(&[json!({"timestamp":future,"engine":"SQLite","operation":"read","statement":"SELECT","recognition":"recorded_code"})], &[], now).unwrap();
    drop(store);
    let service = Service::start(home.path(), false, None);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if service.get("/api/history?section=sql").json()["total"] == 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "future activity was not promoted"
        );
        std::thread::sleep(Duration::from_millis(40));
    }
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
    let matching = |stream: &mut EventStream, require_cancelled: bool| {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let frame = stream.snapshot();
            let http = service.get("/api/snapshot?window=1h").json();
            let has_cancelled = frame["logs"]["entries"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["code"] == "cancelled"));
            if frame["snapshot"]["updated_at"] == http["updated_at"]
                && (!require_cancelled || has_cancelled)
            {
                return (frame, http);
            }
            assert!(
                Instant::now() < deadline,
                "matching snapshot was not published"
            );
        }
    };
    let (first, mut first_http_snapshot) = matching(&mut stream, false);
    assert_eq!(first["window"], "1h");
    let mut first_stream_snapshot = first["snapshot"].clone();
    for snapshot in [&mut first_stream_snapshot, &mut first_http_snapshot] {
        let monitor = snapshot["monitor"].as_object_mut().unwrap();
        for key in [
            "collection",
            "requests",
            "http_errors",
            "snapshot_bytes",
            "transfer_bytes",
            "process_usage",
            "uptime_seconds",
            "device_checked_at",
            "cpu_usage_percent",
        ] {
            monitor.remove(key);
        }
    }
    assert_eq!(first_stream_snapshot, first_http_snapshot);
    assert_eq!(first["logs"], service.get("/api/logs?window=1h").json());
    let session_logs = |logs: &Value| {
        logs["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["source"] == "session")
            .cloned()
            .collect::<Vec<_>>()
    };
    let first_session_logs = session_logs(&first["logs"]);
    assert_eq!(first_session_logs.len(), 1);
    assert_eq!(first_session_logs[0]["code"], "initial_failure");
    assert!(first["logs"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["source"] == "monitor" && entry["code"] == "started"));
    assert_eq!(session_logs(&service.get("/api/logs").json()).len(), 2);

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
    let (after, mut after_http_snapshot) = matching(&mut stream, true);
    assert!(after["version"].as_u64().unwrap() > first["version"].as_u64().unwrap());
    let mut after_stream_snapshot = after["snapshot"].clone();
    for snapshot in [&mut after_stream_snapshot, &mut after_http_snapshot] {
        let monitor = snapshot["monitor"].as_object_mut().unwrap();
        for key in [
            "collection",
            "requests",
            "http_errors",
            "snapshot_bytes",
            "transfer_bytes",
            "process_usage",
            "uptime_seconds",
            "device_checked_at",
            "cpu_usage_percent",
        ] {
            monitor.remove(key);
        }
    }
    assert_eq!(after_stream_snapshot, after_http_snapshot);
    assert_eq!(after["logs"], service.get("/api/logs?window=1h").json());
    assert_eq!(session_logs(&after["logs"]).len(), 2);
    assert_eq!(
        after["snapshot"]["errors"]["events"],
        json!(session_logs(&after["logs"]))
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
    let reconnected_snapshot = reconnected.snapshot();
    assert_eq!(reconnected_snapshot["version"], after["version"]);
    assert_eq!(
        reconnected_snapshot["snapshot"]["codex"],
        after["snapshot"]["codex"]
    );
    assert_eq!(reconnected_snapshot["logs"], after["logs"]);
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
    assert_eq!(recovered.snapshot()["logs"]["enabled"], true);
    service.stop();
}

#[test]
fn idle_stream_heartbeats_do_not_republish_the_source() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let mut service = Service::start(home.path(), true, None);
    snapshot_when(&service, |snapshot| {
        snapshot["codex"]["health"] == "ok" && snapshot["monitor"]["device_ready"] == true
    });
    let mut stream = EventStream::connect(&service, "").ok().unwrap();
    let first = stream.snapshot();
    fs::write(home.path().join("sessions/ignored.txt"), "unrelated").unwrap();
    stream
        .0
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(18)))
        .unwrap();
    assert_eq!(stream.frame(), ": heartbeat\n\n");
    assert_eq!(
        service.get("/api/snapshot").json()["codex"],
        first["snapshot"]["codex"]
    );
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
fn model_api_observation_switch_preserves_other_diagnostics_and_replays() {
    let home = tempdir().unwrap();
    native_event_fixture(home.path());
    let stamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let time = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).timestamp();
    let source = rusqlite::Connection::open(home.path().join("logs_fixture.sqlite")).unwrap();
    source
        .execute_batch(
            "CREATE TABLE logs(id INTEGER PRIMARY KEY,ts REAL,level TEXT,target TEXT,message TEXT)",
        )
        .unwrap();
    let request = r#"event.name="codex.api_request" endpoint="/responses" status_code=200"#;
    source
        .execute(
            "INSERT INTO logs VALUES(1,?,'INFO','codex_otel.log_only',?)",
            rusqlite::params![time, request],
        )
        .unwrap();
    let directory = home.path().join("desktop-logs");
    fs::create_dir(&directory).unwrap();
    let desktop = directory.join("codex-desktop-fixture.log");
    fs::write(
        &desktop,
        format!("{stamp} INFO [codex_otel.log_only] {request}\n"),
    )
    .unwrap();
    let service = Service::start(home.path(), true, None);
    snapshot_when(&service, |value| value["model_api"]["requests"] == 2);
    let set = |value: Value| {
        let body = value.to_string();
        service.request_body(&format!("POST /api/settings HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", service.port, body.len()), &body)
    };
    let response = set(json!({"observations":{"model_api":false}}));
    assert_eq!(response.status, 200);
    assert_eq!(response.json()["observations"]["model_api"], false);
    let disabled = service.get("/api/snapshot?window=all").json();
    assert_eq!(disabled["model_api"]["enabled"], false);
    assert_eq!(disabled["model_api"]["requests"], 0);
    assert!(disabled["model_api"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|source| source["health"] == "disabled"));
    source
        .execute(
            "INSERT INTO logs VALUES(2,?,'ERROR','codex_otel.log_only',?)",
            rusqlite::params![time, request],
        )
        .unwrap();
    source
        .execute(
            "INSERT INTO logs VALUES(3,?,'INFO','sqlx::query',?)",
            rusqlite::params![time, "sql=\"SELECT 3\" elapsed=1ms"],
        )
        .unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&desktop)
        .unwrap()
        .write_all(format!("{stamp} ERROR [codex_otel.log_only] {request}\n").as_bytes())
        .unwrap();
    let disabled = snapshot_when(&service, |value| {
        value["codex"]["sqlite"]["total"] == 1
            && value["errors"]["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["source"] == "codex_core" && event["record_id"] == 2)
                    && events.iter().any(|event| {
                        event["source"] == "codex_desktop"
                            && event["module"] == "codex_otel.log_only"
                    })
            })
    });
    assert_eq!(disabled["model_api"]["requests"], 0);
    assert_eq!(
        set(json!({"observations":{"model_api":true,"errors":true}})).status,
        200
    );
    let restored = snapshot_when(&service, |value| value["model_api"]["requests"] == 4);
    assert_eq!(restored["model_api"]["enabled"], true);
    assert_eq!(restored["codex"]["sqlite"]["total"], 1);
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
