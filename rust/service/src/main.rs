mod assets;
mod dots;
mod events;
mod history;
mod journal;
mod performance_debug;
mod server;
mod sessions;

use std::env;
use std::error::Error;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const USAGE: &str = "local-activity-monitor [--codex-home PATH] [--port PORT] [--codex] [--open] [--assets PATH] [--watch-stdin] [--prepare]";

fn default_codex_home(
    configured: Option<std::ffi::OsString>,
    user_home: Option<PathBuf>,
) -> Option<PathBuf> {
    configured
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| user_home.map(|path| path.join(".codex")))
}

#[cfg(windows)]
fn platform_home() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(windows))]
fn platform_home() -> Option<PathBuf> {
    env::home_dir()
}

struct Options {
    home: PathBuf,
    port: u16,
    codex: bool,
    open: bool,
    assets: Option<PathBuf>,
    watch_stdin: bool,
    prepare: bool,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        Self::parse_from(
            env::args_os().skip(1),
            default_codex_home(env::var_os("CODEX_HOME"), platform_home()),
        )
    }

    fn parse_from<I: Iterator<Item = std::ffi::OsString>>(
        mut args: I,
        mut home: Option<PathBuf>,
    ) -> Result<Option<Self>, String> {
        let mut port = 8787;
        let mut codex = false;
        let mut open = false;
        let mut assets = None;
        let mut watch_stdin = false;
        let mut prepare = false;
        while let Some(arg) = args.next() {
            let value = |args: &mut I| {
                args.next()
                    .ok_or_else(|| format!("Missing value for {}", arg.to_string_lossy()))
            };
            match arg.to_str() {
                Some("--help" | "-h") => return Ok(None),
                Some("--codex-home") => home = Some(value(&mut args)?.into()),
                Some("--port") => {
                    port = value(&mut args)?
                        .to_str()
                        .and_then(|value| value.parse().ok())
                        .ok_or_else(|| String::from("Invalid port"))?;
                }
                Some("--codex") => codex = true,
                Some("--open") => open = true,
                Some("--assets") => assets = Some(value(&mut args)?.into()),
                Some("--watch-stdin") => watch_stdin = true,
                Some("--prepare") => prepare = true,
                _ => return Err(String::from("Unsupported argument")),
            }
        }
        let home = home
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| String::from("Select a Codex home with --codex-home or CODEX_HOME"))?;
        if prepare && !watch_stdin {
            return Err("--prepare requires --watch-stdin".into());
        }
        Ok(Some(Self {
            home,
            port,
            codex,
            open,
            assets,
            watch_stdin,
            prepare,
        }))
    }
}

fn open_browser(url: &str) {
    let configured = env::var("BROWSER").ok();
    if let Some(choices) = configured.as_deref() {
        let separator = if cfg!(windows) { ';' } else { ':' };
        for choice in choices.split(separator).filter(|choice| !choice.is_empty()) {
            let Some(mut command) = (if choice.contains("%s") {
                shlex::split(choice)
            } else {
                Some(vec![choice.to_owned(), "%s".to_owned()])
            }) else {
                continue;
            };
            if command.is_empty() {
                continue;
            }
            let background = command.last().is_some_and(|arg| arg == "&");
            if background {
                command.pop();
            }
            if command.is_empty() {
                continue;
            }
            let mut process = std::process::Command::new(&command[0]);
            process.args(command[1..].iter().map(|arg| arg.replace("%s", url)));
            if background {
                if let Ok(mut child) = process.spawn() {
                    match child.try_wait() {
                        Ok(Some(status)) if status.success() => return,
                        Ok(Some(_)) => {}
                        _ => {
                            std::thread::spawn(move || {
                                let _ = child.wait();
                            });
                            return;
                        }
                    }
                }
            } else if process.status().is_ok_and(|status| status.success()) {
                return;
            }
        }
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    if configured.is_some() {
        for (program, prefix) in [
            ("xdg-open", None),
            ("gio", Some("open")),
            ("x-www-browser", None),
        ] {
            let mut command = std::process::Command::new(program);
            command.env_remove("BROWSER");
            if let Some(prefix) = prefix {
                command.arg(prefix);
            }
            if command
                .arg(url)
                .status()
                .is_ok_and(|status| status.success())
            {
                return;
            }
        }
        return;
    }
    let _ = webbrowser::open(url);
}

fn home_identity(home: &Path) -> io::Result<String> {
    let path = home
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid home path"))?;
    #[cfg(windows)]
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}").to_lowercase()
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_lowercase()
    };
    Ok(format!("{:x}", Sha256::digest(path.as_bytes()))[..24].into())
}

async fn existing_instance(port: u16, home_id: &str) -> Option<String> {
    if port == 0 {
        return None;
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = tokio::net::TcpStream::connect(address).await.ok()?;
        let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
            .max_buf_size(8192)
            .handshake(TokioIo::new(stream))
            .await
            .ok()?;
        let request = Request::builder()
            .uri("/api/instance")
            .header("host", format!("127.0.0.1:{port}"))
            .header("connection", "close")
            .body(Empty::<Bytes>::new())
            .ok()?;
        let response = async {
            let response = sender.send_request(request).await.ok()?;
            if response.status() != StatusCode::OK {
                return None;
            }
            let mut body = response.into_body();
            let mut bytes = Vec::new();
            while let Some(frame) = body.frame().await {
                if let Ok(data) = frame.ok()?.into_data() {
                    if bytes.len() + data.len() > 4097 {
                        return None;
                    }
                    bytes.extend_from_slice(&data);
                }
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            (value["application"] == "local-activity-monitor" && value["home_id"] == home_id)
                .then(|| format!("http://127.0.0.1:{port}/"))
        };
        tokio::pin!(connection);
        tokio::pin!(response);
        tokio::select! {
            biased;
            result = &mut response => result,
            _ = &mut connection => response.await,
        }
    })
    .await
    .ok()
    .flatten()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Start,
    Pause,
    Resume,
    Stop,
    Restart,
    LauncherClosed,
}

fn parse_control(bytes: &[u8]) -> Option<Control> {
    match std::str::from_utf8(bytes).ok()?.trim() {
        "start" => Some(Control::Start),
        "pause" => Some(Control::Pause),
        "resume" => Some(Control::Resume),
        "restart" => Some(Control::Restart),
        _ => None,
    }
}

#[derive(Default)]
struct ControlLine {
    command: Vec<u8>,
    utf8: Vec<u8>,
    trailing_space: bool,
    invalid: bool,
}

impl ControlLine {
    fn finish(&self) -> Option<Control> {
        if self.invalid || !self.utf8.is_empty() {
            return None;
        }
        parse_control(&self.command)
    }

    fn push(&mut self, byte: u8) -> Option<Control> {
        if byte == b'\n' {
            let command = self.finish();
            *self = Self::default();
            return command;
        }
        if self.invalid {
            return None;
        }
        self.utf8.push(byte);
        let character = match std::str::from_utf8(&self.utf8) {
            Ok(text) => text.chars().next().expect("nonempty character"),
            Err(error) if error.error_len().is_none() && self.utf8.len() < 4 => return None,
            Err(_) => {
                self.invalid = true;
                self.utf8.clear();
                return None;
            }
        };
        self.utf8.clear();
        if character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}') {
            self.trailing_space |= !self.command.is_empty();
        } else if self.trailing_space || !character.is_ascii() || self.command.len() == 7 {
            self.invalid = true;
        } else {
            self.command.push(character as u8);
        }
        None
    }
}

fn stdin_control() -> mpsc::Receiver<Control> {
    let (signal, done) = mpsc::channel(16);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        let mut chunk = [0; 1024];
        let mut line = ControlLine::default();
        loop {
            let count = match input.read(&mut chunk) {
                Ok(0) => {
                    if let Some(command) = line.finish() {
                        let _ = signal.blocking_send(command);
                        if command == Control::Restart {
                            return;
                        }
                    }
                    let _ = signal.blocking_send(Control::LauncherClosed);
                    return;
                }
                Err(_) => {
                    let _ = signal.blocking_send(Control::Stop);
                    return;
                }
                Ok(count) => count,
            };
            for byte in &chunk[..count] {
                if let Some(command) = line.push(*byte) {
                    if signal.blocking_send(command).is_err() || command == Control::Restart {
                        return;
                    }
                }
            }
        }
    });
    done
}

async fn wait_control(receiver: &mut Option<mpsc::Receiver<Control>>, paused: bool) -> Control {
    loop {
        let command = tokio::select! {
            _ = tokio::signal::ctrl_c() => Control::Stop,
            value = async {
                match receiver {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }
            } => value.unwrap_or(Control::Stop),
        };
        match command {
            Control::Stop | Control::Restart | Control::LauncherClosed => return command,
            Control::Pause if !paused => return Control::Pause,
            Control::Resume if paused => return Control::Resume,
            _ => {}
        }
    }
}

async fn record_control(action: Control, journal: Arc<std::sync::Mutex<journal::Journal>>) {
    let kind = match action {
        Control::Restart => "restart_requested",
        Control::LauncherClosed => "launcher_closed",
        _ => return,
    };
    let _ = tokio::task::spawn_blocking(move || {
        journal
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .event(kind)
    })
    .await;
}

fn load_assets(options: &Options) -> io::Result<Option<assets::Assets>> {
    options
        .assets
        .as_deref()
        .map(assets::Assets::load)
        .transpose()
}

async fn run(options: Options) -> Result<(), Box<dyn Error>> {
    let home = options.home.canonicalize()?;
    if !home.is_dir() {
        return Err("Codex home must be a directory".into());
    }
    let home_id = home_identity(&home)?;
    let mut commands = options.prepare.then(stdin_control);
    let prepared_assets = if let Some(receiver) = &mut commands {
        let assets = load_assets(&options)?;
        println!("{}", json!({"status":"source_ready"}));
        io::stdout().flush()?;
        let start = tokio::select! {
            _ = tokio::signal::ctrl_c() => false,
            command = receiver.recv() => command == Some(Control::Start),
        };
        if !start {
            return Ok(());
        }
        Some(assets)
    } else {
        None
    };
    if let Some(url) = existing_instance(options.port, &home_id).await {
        println!("{}", json!({"status":"already_running","url":url}));
        io::stdout().flush()?;
        if options.open {
            open_browser(&url);
        }
        return Ok(());
    }
    let assets = match prepared_assets {
        Some(assets) => assets,
        None => load_assets(&options)?,
    };
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, options.port))).await?;
    let monitoring = home.join("monitoring");
    let upgrade_path = monitoring.clone();
    let codex = options.codex;
    tokio::task::spawn_blocking(move || prepare_history(&upgrade_path, codex)).await?;
    let cleanup_path = monitoring.clone();
    let cleanup_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cleanup_flag = std::sync::Arc::clone(&cleanup_stop);
    let (monitor, snapshots) = sessions::Monitor::start(
        &home,
        options.codex,
        assets.as_ref().map(|assets| assets.revision.clone()),
    )?;
    let state = Arc::new(server::State {
        home_id,
        monitoring,
        codex: options.codex,
        assets,
        snapshots,
        refresh: monitor.refresh_sender(),
        metadata_enabled: monitor.metadata_setting(),
        metadata_lock: tokio::sync::Mutex::new(()),
        journal: monitor.journal(),
        debug: monitor.debug(),
        event_clients: Arc::new(tokio::sync::Semaphore::new(events::CLIENT_LIMIT)),
    });
    let refresh = monitor.refresh_sender();
    let _cleanup = CleanupWorker {
        stop: cleanup_stop,
        handle: Some(std::thread::spawn(move || {
            cleanup_history(&cleanup_path, &cleanup_flag, refresh);
        })),
    };
    let address = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}/", address.port());
    if options.watch_stdin && commands.is_none() {
        commands = Some(stdin_control());
    }
    let mut listener = listener;
    let mut open = options.open;
    loop {
        println!(
            "{}",
            json!({"status":"listening","url":url,"codex_metadata":options.codex})
        );
        io::stdout().flush()?;
        if open {
            open_browser(&url);
            open = false;
        }
        let action = server::serve(
            listener,
            Arc::clone(&state),
            wait_control(&mut commands, false),
        )
        .await?;
        if matches!(
            action,
            Control::Stop | Control::Restart | Control::LauncherClosed
        ) {
            record_control(action, Arc::clone(&state.journal)).await;
            break;
        }
        println!("{}", json!({"status":"paused"}));
        io::stdout().flush()?;
        let action = wait_control(&mut commands, true).await;
        if matches!(
            action,
            Control::Stop | Control::Restart | Control::LauncherClosed
        ) {
            record_control(action, Arc::clone(&state.journal)).await;
            break;
        }
        listener = TcpListener::bind(address).await?;
    }
    Ok(())
}

fn prepare_history(directory: &Path, codex: bool) {
    use lam_history_store::{Namespace, Store};

    if let Err(error) = Store::upgrade(directory) {
        eprintln!("History upgrade unavailable: {error}");
        return;
    }
    let now = std::time::SystemTime::now().into();
    for namespace in [Namespace::Activity, Namespace::Errors, Namespace::Threads] {
        if namespace == Namespace::Threads && !codex {
            continue;
        }
        if let Err(error) = Store::import_legacy(directory, namespace, now) {
            eprintln!("Legacy history import unavailable ({namespace:?}): {error}");
        }
    }
}

struct CleanupWorker {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for CleanupWorker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            let _ = handle.join();
        }
    }
}

fn cleanup_history(
    directory: &Path,
    stop: &std::sync::atomic::AtomicBool,
    refresh: Option<(
        std::sync::mpsc::SyncSender<()>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    )>,
) {
    use lam_history_store::{Access, Store};
    use std::sync::atomic::Ordering;

    let cleanup = || -> lam_history_store::Result<()> {
        let Some(reader) = Store::open(directory, Access::ReadOnly)? else {
            return Ok(());
        };
        let days = reader.activity_retention()?;
        drop(reader);
        if days == 0 {
            return Ok(());
        }
        let Some(mut writer) = Store::open(directory, Access::ReadWrite)? else {
            return Ok(());
        };
        while !stop.load(Ordering::Acquire) {
            let removed =
                writer.continue_retention_cleanup(days, std::time::SystemTime::now().into())?;
            if removed > 0 {
                if let Some((wake, manual)) = &refresh {
                    manual.store(true, Ordering::Release);
                    let _ = wake.try_send(());
                }
            }
            if removed < 500 {
                break;
            }
            std::thread::yield_now();
        }
        Ok(())
    };
    while !stop.load(Ordering::Acquire) {
        match cleanup() {
            Ok(()) => break,
            Err(lam_history_store::StoreError::Sql(rusqlite::Error::SqliteFailure(error, _)))
                if matches!(
                    error.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                std::thread::park_timeout(Duration::from_secs(1));
            }
            Err(error) => {
                eprintln!("History cleanup unavailable: {error}");
                break;
            }
        }
    }
}

fn main() {
    let options = match Options::parse() {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{USAGE}");
            return;
        }
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(8)
        .build()
        .map_err(Box::<dyn Error>::from)
        .and_then(|runtime| {
            let result = runtime.block_on(run(options));
            runtime.shutdown_timeout(Duration::from_secs(2));
            result
        });
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_lines_accept_long_whitespace_and_bound_invalid_input() {
        let mut line = ControlLine::default();
        for byte in " "
            .repeat(100_000)
            .bytes()
            .chain("\u{2003}pause\u{1c}".bytes())
        {
            assert!(line.push(byte).is_none());
            assert!(line.command.len() <= 7);
            assert!(line.utf8.len() <= 3);
        }
        assert!(matches!(line.push(b'\n'), Some(Control::Pause)));
        for byte in "x".repeat(100_000).bytes() {
            assert!(line.push(byte).is_none());
            assert!(line.command.len() <= 7);
            assert!(line.utf8.len() <= 3);
        }
        assert!(line.push(b'\n').is_none());
        for byte in b"resume" {
            assert!(line.push(*byte).is_none());
        }
        assert!(matches!(line.finish(), Some(Control::Resume)));
    }

    #[test]
    fn cli_defaults_match_python_and_explicit_options_take_precedence() {
        let profile = PathBuf::from("/fixture/profile");
        assert_eq!(
            default_codex_home(None, Some(profile.clone())),
            Some(profile.join(".codex"))
        );
        assert_eq!(
            default_codex_home(Some("".into()), Some(profile.clone())),
            Some(profile.join(".codex"))
        );
        assert_eq!(
            default_codex_home(Some("/fixture/selected".into()), Some(profile.clone())),
            Some(PathBuf::from("/fixture/selected"))
        );
        assert_eq!(default_codex_home(None, None), None);
        let defaults = Options::parse_from(std::iter::empty(), Some(profile.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(defaults.home, profile);
        assert_eq!(defaults.port, 8787);
        assert!(!defaults.codex);
        let explicit = Options::parse_from(
            [
                "--codex-home",
                "/fixture/override",
                "--port",
                "0",
                "--codex",
            ]
            .into_iter()
            .map(Into::into),
            Some(profile),
        )
        .unwrap()
        .unwrap();
        assert_eq!(explicit.home, PathBuf::from("/fixture/override"));
        assert_eq!(explicit.port, 0);
        assert!(explicit.codex);
        assert!(Options::parse_from(
            ["--codex-home", ""].into_iter().map(Into::into),
            Some(PathBuf::from("/fixture/profile")),
        )
        .is_err());
    }
}
