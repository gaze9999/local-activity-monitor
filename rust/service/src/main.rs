mod assets;
mod dots;
mod events;
mod gpu_info;
#[cfg(windows)]
mod gpu_windows;
mod history;
mod journal;
mod performance_debug;
mod process_usage;
mod runtime_info;
mod server;
mod sessions;
mod sse_write;
mod system_fonts;

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

const USAGE: &str = "local-activity-monitor [--codex-home PATH] [--port PORT] [--max-files COUNT] [--codex] [--open] [--assets PATH] [--watch-stdin] [--prepare] [--enable-jev | --disable-jev] [--database PATH] [--configure-only]";

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

#[cfg(windows)]
fn protect_children() -> io::Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // The unnamed job handle is deliberately retained until process teardown.
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        ) == 0
            || AssignProcessToJobObject(job, GetCurrentProcess()) == 0
        {
            let error = io::Error::last_os_error();
            CloseHandle(job);
            return Err(error);
        }
    }
    Ok(())
}

struct Options {
    home: PathBuf,
    port: u16,
    max_files: u32,
    codex: bool,
    open: bool,
    assets: Option<PathBuf>,
    watch_stdin: bool,
    prepare: bool,
    jev_recording: Option<bool>,
    jev_database: Option<PathBuf>,
    configure_only: bool,
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
        let mut max_files = 20;
        let mut codex = false;
        let mut open = false;
        let mut assets = None;
        let mut watch_stdin = false;
        let mut prepare = false;
        let mut jev_recording = None;
        let mut jev_database = None;
        let mut configure_only = false;
        while let Some(arg) = args.next() {
            let inline = arg.to_str().and_then(|text| text.split_once('=')).filter(|(name, _)| {
                matches!(*name, "--codex-home" | "--port" | "--max-files" | "--assets" | "--database")
            });
            let name = inline.map(|(name, _)| name).or_else(|| arg.to_str());
            let value = |args: &mut I| {
                inline.map(|(_, value)| std::ffi::OsString::from(value)).or_else(|| args.next())
                    .ok_or_else(|| format!("Missing value for {}", arg.to_string_lossy()))
            };
            match name {
                Some("--help" | "-h") => return Ok(None),
                Some("--codex-home") => home = Some(value(&mut args)?.into()),
                Some("--port") => {
                    port = value(&mut args)?
                        .to_str()
                        .and_then(history::query_number)
                        .and_then(|value| u16::try_from(value).ok())
                        .ok_or_else(|| String::from("Invalid port"))?;
                }
                Some("--max-files") => {
                    max_files = value(&mut args)?
                        .to_str()
                        .and_then(history::query_number)
                        .filter(|value| (1..=5000).contains(value))
                        .ok_or_else(|| String::from("Invalid max-files"))?;
                }
                Some("--codex") => codex = true,
                Some("--open") => open = true,
                Some("--assets") => assets = Some(value(&mut args)?.into()),
                Some("--watch-stdin") => watch_stdin = true,
                Some("--prepare") => prepare = true,
                Some("--enable-jev") => {
                    if jev_recording == Some(false) {
                        return Err("Conflicting Jev recording options".into());
                    }
                    jev_recording = Some(true);
                }
                Some("--disable-jev") => {
                    if jev_recording == Some(true) {
                        return Err("Conflicting Jev recording options".into());
                    }
                    jev_recording = Some(false);
                }
                Some("--database") => jev_database = Some(value(&mut args)?.into()),
                Some("--configure-only") => configure_only = true,
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
            max_files,
            codex,
            open,
            assets,
            watch_stdin,
            prepare,
            jev_recording,
            jev_database,
            configure_only,
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

fn resolve_home(path: &Path) -> io::Result<PathBuf> {
    resolve_home_with_depth(path, 0)
}

fn resolve_home_with_depth(path: &Path, depth: u8) -> io::Result<PathBuf> {
    if depth > 40 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Too many home path links"));
    }
    let absolute = if path.is_absolute() { path.to_path_buf() } else { env::current_dir()?.join(path) };
    match absolute.canonicalize() {
        Ok(path) => return Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }
    let mut resolved = PathBuf::new();
    let mut components = absolute.components();
    while let Some(component) = components.next() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => resolved.push(component),
            std::path::Component::CurDir => {},
            std::path::Component::ParentDir => { resolved.pop(); },
            std::path::Component::Normal(name) => {
                let candidate = resolved.join(name);
                match candidate.canonicalize() {
                    Ok(path) => resolved = path,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        match std::fs::symlink_metadata(&candidate) {
                            Err(missing) if missing.kind() == io::ErrorKind::NotFound => resolved = candidate,
                            Ok(metadata) if metadata.file_type().is_symlink() => {
                                let target = std::fs::read_link(&candidate)?;
                                let target = if target.is_absolute() { target } else { resolved.join(target) };
                                return resolve_home_with_depth(&target.join(components.as_path()), depth + 1);
                            }
                            _ => return Err(error),
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
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

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = signal(SignalKind::terminate()).ok();
        let mut hangup = signal(SignalKind::hangup()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = async { match &mut terminate { Some(signal) => { signal.recv().await; }, None => std::future::pending().await } } => {},
            _ = async { match &mut hangup { Some(signal) => { signal.recv().await; }, None => std::future::pending().await } } => {},
        }
    }
    #[cfg(windows)]
    {
        let mut ctrl_break = tokio::signal::windows::ctrl_break().ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = async { match &mut ctrl_break { Some(signal) => { signal.recv().await; }, None => std::future::pending().await } } => {},
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn wait_control(receiver: &mut Option<mpsc::Receiver<Control>>, paused: bool) -> Control {
    loop {
        let command = tokio::select! {
            _ = shutdown_signal() => Control::Stop,
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
    let selected_home = assets::expand_development_path(options.home.clone())?;
    if let Some(enabled) = options.jev_recording {
        let home = if selected_home.is_absolute() {
            selected_home.clone()
        } else {
            env::current_dir()?.join(&selected_home)
        };
        let (changed, _) = sessions::jev::configure(
            &home.join("monitoring"),
            enabled,
            options.jev_database.as_deref(),
        )
        .map_err(|error| io::Error::other(format!("Invalid Jev configuration: {error:?}")))?;
        println!(
            "{}",
            json!({"jev_recording":if !changed {"unchanged"} else if enabled {"enabled"} else {"disabled"}, "reload_existing_jev_processes":true})
        );
        io::stdout().flush()?;
    }
    if options.configure_only {
        return Ok(());
    }
    let home = resolve_home(&selected_home)?;
    match std::fs::metadata(&home) {
        Ok(metadata) if metadata.is_dir() => {},
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Ok(_) => return Err("Codex home must be a directory".into()),
        Err(error) => return Err(error.into()),
    }
    let home_id = home_identity(&home)?;
    let mut commands = options.prepare.then(stdin_control);
    let prepared_assets = if let Some(receiver) = &mut commands {
        let assets = load_assets(&options)?;
        println!("{}", json!({"status":"source_ready"}));
        io::stdout().flush()?;
        let start = tokio::select! {
            _ = shutdown_signal() => false,
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
    let (cleanup_wake, cleanup_signal) = std::sync::mpsc::sync_channel(1);
    let (monitor, snapshots) = sessions::Monitor::start(
        &home,
        options.codex,
        assets.as_ref().map(|assets| assets.revision.clone()),
        options.max_files,
    )?;
    let font_output = monitor.fonts();
    let font_completion = monitor.device_completion();
    let font_wake = monitor.refresh_sender();
    let mut font_worker = AccountWorker(Some(tokio::spawn(async move {
        let fonts = system_fonts::collect().await;
        *font_output
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(fonts);
        font_completion
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .fonts_finished();
        if let Some((wake, manual)) = font_wake {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
    })));
    let gpu_output = monitor.gpus();
    let gpu_completion = monitor.device_completion();
    let gpu_wake = monitor.refresh_sender();
    let mut gpu_worker = AccountWorker(Some(tokio::spawn(async move {
        let gpus = gpu_info::collect().await;
        *gpu_output
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some(gpus);
        gpu_completion
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .gpus_finished();
        if let Some((wake, manual)) = gpu_wake {
            manual.store(true, std::sync::atomic::Ordering::Release);
            let _ = wake.try_send(());
        }
    })));
    let account = Arc::new(sessions::account::Source::new(
        home.clone(),
        monitor.account_shared(),
    ));
    let mut account_worker = AccountWorker(Some(tokio::spawn(Arc::clone(&account).run())));
    let cloud = Arc::new(sessions::cloud::Source::new(
        home.clone(),
        monitor.cloud_shared(),
    ));
    let mut cloud_worker = AccountWorker(Some(tokio::spawn(Arc::clone(&cloud).run())));
    let [public_openai_shared, public_codex_shared] = monitor.public_shared();
    let public_openai = Arc::new(sessions::public_api::Source::new(
        sessions::public_api::Kind::Openai,
        public_openai_shared,
    )?);
    let public_codex = Arc::new(sessions::public_api::Source::new(
        sessions::public_api::Kind::Codex,
        public_codex_shared,
    )?);
    let mut public_openai_worker =
        AccountWorker(Some(tokio::spawn(Arc::clone(&public_openai).run())));
    let mut public_codex_worker =
        AccountWorker(Some(tokio::spawn(Arc::clone(&public_codex).run())));
    let platform_shared = monitor.platform_shared();
    let mut platform_sources = Vec::new();
    let mut platform_workers = Vec::new();
    for (kind, shared) in [
        sessions::platform::Kind::Models,
        sessions::platform::Kind::Usage,
        sessions::platform::Kind::Decisions,
        sessions::platform::Kind::Costs,
    ]
    .into_iter()
    .zip(platform_shared)
    {
        let source = Arc::new(sessions::platform::Source::new(kind, shared)?);
        platform_workers.push(AccountWorker(Some(tokio::spawn(Arc::clone(&source).run()))));
        platform_sources.push(source);
    }
    let state = Arc::new(server::State {
        home_id,
        monitoring,
        codex: monitor.codex_setting(),
        traffic: monitor.traffic(),
        account,
        cloud,
        public_openai,
        public_codex,
        platform_sources,
        assets,
        snapshots,
        refresh: monitor.refresh_sender(),
        metadata_enabled: monitor.metadata_setting(),
        tool_events_enabled: monitor.tool_events_setting(),
        files_enabled: monitor.files_setting(),
        errors_enabled: monitor.errors_setting(),
        sqlite_enabled: monitor.sqlite_setting(),
        model_api_enabled: monitor.model_api_setting(),
        command_switches: monitor.command_switches_setting(),
        mcp_sources: monitor.mcp_sources_setting(),
        mcp_switches: monitor.mcp_switches_setting(),
        customizations: monitor.customizations_setting(),
        tracking: monitor.tracking_setting(),
        cleanup_wake: cleanup_wake.clone(),
        metadata_lock: tokio::sync::Mutex::new(()),
        journal: monitor.journal(),
        debug: monitor.debug(),
        details: monitor.detail_index(),
        core_source: monitor.core_source(),
        desktop_sources: monitor.desktop_sources(),
        event_clients: Arc::new(tokio::sync::Semaphore::new(events::CLIENT_LIMIT)),
    });
    let refresh = monitor.refresh_sender();
    let _cleanup = CleanupWorker {
        stop: cleanup_stop,
        wake: cleanup_wake,
        handle: Some(std::thread::spawn(move || {
            cleanup_history(&cleanup_path, &cleanup_flag, refresh, cleanup_signal);
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
    account_worker.stop().await;
    cloud_worker.stop().await;
    public_openai_worker.stop().await;
    public_codex_worker.stop().await;
    for worker in &mut platform_workers {
        worker.stop().await;
    }
    font_worker.finish().await;
    gpu_worker.finish().await;
    Ok(())
}

struct AccountWorker(Option<tokio::task::JoinHandle<()>>);
impl AccountWorker {
    async fn finish(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.await;
        }
    }
    async fn stop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
}
impl Drop for AccountWorker {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
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
    wake: std::sync::mpsc::SyncSender<()>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for CleanupWorker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = self.wake.try_send(());
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
    signal: std::sync::mpsc::Receiver<()>,
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
        let delay = match cleanup() {
            Ok(()) => Duration::from_secs(60),
            Err(lam_history_store::StoreError::Sql(rusqlite::Error::SqliteFailure(error, _)))
                if matches!(
                    error.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                Duration::from_secs(1)
            }
            Err(error) => {
                eprintln!("History cleanup unavailable: {error}");
                Duration::from_secs(30)
            }
        };
        match signal.recv_timeout(delay) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
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
    #[cfg(windows)]
    if let Err(error) = protect_children() {
        eprintln!("{error}");
        std::process::exit(1);
    }
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

    #[cfg(unix)]
    #[test]
    fn missing_home_resolves_dangling_symlink_without_creating_it() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("missing-target");
        let link = directory.path().join("dangling");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(resolve_home(&link.join("child")).unwrap(), directory.path().canonicalize().unwrap().join("missing-target/child"));
        assert!(!target.exists());
    }

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
    fn cli_accepts_inline_values_and_python_decimal_integers() {
        let options = Options::parse_from(
            ["--codex-home=/fixture/a=b", "--port=１２", "--max-files=1_0", "--database=/fixture/db=1"]
                .into_iter().map(Into::into),
            None,
        ).unwrap().unwrap();
        assert_eq!(options.home, PathBuf::from("/fixture/a=b"));
        assert_eq!(options.port, 12);
        assert_eq!(options.max_files, 10);
        assert_eq!(options.jev_database, Some(PathBuf::from("/fixture/db=1")));
        for invalid in ["--port=65536", "--port=1__2", "--max-files=0", "--codex=true"] {
            assert!(Options::parse_from([invalid].into_iter().map(Into::into), Some(PathBuf::from("/fixture"))).is_err());
        }
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
        assert_eq!(defaults.max_files, 20);
        assert!(!defaults.codex);
        let explicit = Options::parse_from(
            [
                "--codex-home",
                "/fixture/override",
                "--port",
                "0",
                "--codex",
                "--max-files",
                "7",
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
        assert_eq!(explicit.max_files, 7);
        for invalid in ["0", "5001", "-1", "abc"] {
            assert!(Options::parse_from(
                ["--max-files", invalid].into_iter().map(Into::into),
                Some(PathBuf::from("/fixture/profile"))
            )
            .is_err());
        }
        assert!(Options::parse_from(
            ["--codex-home", ""].into_iter().map(Into::into),
            Some(PathBuf::from("/fixture/profile")),
        )
        .is_err());
    }
}
