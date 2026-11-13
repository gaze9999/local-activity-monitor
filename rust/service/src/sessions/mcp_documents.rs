//! Discover only documents declared by an enabled local MCP launcher.
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const READ_LIMIT: usize = 65_536;
const EDIT_LIMIT: usize = 32_768;
static JS_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"['\"]([A-Za-z0-9_.-]{1,160}\.(?:yml|yaml|json|toml|txt|md))['\"]"#).unwrap()
});
static DATA_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(terms|rules|config|settings)").unwrap());

fn safe_file(path: &Path) -> Option<fs::Metadata> {
    if !super::source_introduction::safe(path) {
        return None;
    }
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || super::redirected(&meta) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            return None;
        }
    }
    #[cfg(windows)]
    {
        let file = super::open_source(path.parent()?, path).ok()?;
        let actual = file.metadata().ok()?;
        if super::redirected(&actual)
            || super::file_identity(&meta) != super::file_identity(&actual)
            || !single_link(&file)
        {
            return None;
        }
    }
    Some(meta)
}

#[cfg(windows)]
fn single_link(file: &fs::File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // The handle belongs to the open file and remains valid for this call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return false;
    }
    unsafe { info.assume_init().nNumberOfLinks == 1 }
}

fn read_bounded(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let before = safe_file(path)?;
    let mut file = super::open_source(path.parent()?, path).ok()?;
    let meta = file.metadata().ok()?;
    #[cfg(windows)]
    if super::redirected(&meta) || !single_link(&file) {
        return None;
    }
    if super::file_identity(&before) != super::file_identity(&meta) {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    let after = fs::symlink_metadata(path).ok()?;
    if super::redirected(&after)
        || super::file_identity(&after) != super::file_identity(&meta)
        || after.len() != meta.len()
        || after.modified().ok() != meta.modified().ok()
    {
        return None;
    }
    #[cfg(windows)]
    if !single_link(&file) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 || after.nlink() > 1 {
            return None;
        }
    }
    Some(bytes)
}

pub(crate) fn configuration(
    home: &Path,
    server: &str,
) -> Option<toml::map::Map<String, toml::Value>> {
    if server.is_empty()
        || server.len() > 80
        || !server
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return None;
    }
    let config = home.join("config.toml");
    let bytes = read_bounded(&config, 1024 * 1024)?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).ok()?;
    let value = text.parse::<toml::Value>().ok()?;
    let servers = value.get("mcp_servers")?.as_table()?;
    let (name, table) = servers
        .iter()
        .take(64)
        .find(|(name, _)| name.as_str() == server)?;
    let _ = name;
    let table = table.as_table()?;
    if table
        .get("enabled")
        .is_some_and(|enabled| enabled.as_bool() != Some(true))
    {
        return None;
    }
    Some(table.clone())
}

fn data_type(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            ["json", "yaml", "yml", "toml", "txt", "md"]
                .contains(&extension.to_ascii_lowercase().as_str())
        })
}

fn add(candidates: &mut Vec<(PathBuf, bool)>, path: PathBuf, editable: bool) {
    if !candidates.iter().any(|(old, _)| old == &path) {
        candidates.push((path, editable));
    }
}

pub(crate) fn documents(table: &toml::map::Map<String, toml::Value>) -> Vec<Value> {
    let mut candidates = Vec::new();
    let mut parents = Vec::new();
    let args = table
        .get("args")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .take(32)
        .filter_map(toml::Value::as_str);
    let env = table
        .get("env")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(|(_, value)| value.as_str());
    for item in args.chain(env).filter(|value| value.chars().count() <= 512) {
        let path = PathBuf::from(item);
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let js = filename.ends_with("config.js")
            || filename.ends_with("config.cjs")
            || filename.ends_with("rc.js")
            || filename.ends_with("rc.cjs");
        if safe_file(&path).is_none() || (!data_type(&path) && !js) {
            continue;
        }
        add(&mut candidates, path.clone(), data_type(&path));
        if let Some(parent) = path.parent() {
            parents.push(parent.to_path_buf());
        }
        if js {
            if let Some(bytes) = read_bounded(&path, READ_LIMIT) {
                if let Ok(body) = std::str::from_utf8(&bytes[..bytes.len().min(READ_LIMIT)]) {
                    for captures in JS_LINK.captures_iter(body).take(32) {
                        if let Some(parent) = path.parent() {
                            add(&mut candidates, parent.join(&captures[1]), true);
                        }
                    }
                }
            }
        }
    }
    let roots = super::source_introduction::roots(table);
    for root in &roots {
        for filename in ["README.md", "readme.md", "package.json", "pyproject.toml"] {
            add(&mut candidates, root.join(filename), false);
        }
    }
    let mut scan_roots = Vec::new();
    for root in parents.into_iter().chain(roots) {
        if !scan_roots.contains(&root) && scan_roots.len() < 16 {
            scan_roots.push(root);
        }
    }
    for root in scan_roots {
        for dir in [root.clone(), root.join("config"), root.join("rules")] {
            if !super::source_introduction::safe(&dir) {
                continue;
            }
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.take(200).flatten() {
                    let path = entry.path();
                    let stem = path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("");
                    if data_type(&path) && DATA_NAME.is_match(stem) {
                        add(&mut candidates, path, true);
                    }
                }
            }
        }
    }
    candidates.into_iter().take(64).filter_map(|(path,editable)| {
        let meta=safe_file(&path)?;
        let text=path.to_str()?;
        let id=format!("{:x}",Sha256::digest(text.as_bytes()));
        let modified=meta.modified().ok().map(chrono::DateTime::<chrono::Utc>::from).map(|time|time.to_rfc3339());
        Some(json!({"id":&id[..32],"name":path.file_name()?.to_str()?,"path":text,"bytes":meta.len(),"modified_at":modified,"format":path.extension().and_then(|value|value.to_str()).unwrap_or(""),"editable":editable && meta.len() <= READ_LIMIT as u64}))
    }).collect()
}

pub(crate) fn read_document(
    table: &toml::map::Map<String, toml::Value>,
    id: &str,
) -> Option<Value> {
    let mut entry = documents(table)
        .into_iter()
        .find(|entry| entry["id"] == id)?;
    let path = PathBuf::from(entry["path"].as_str()?);
    let Some(raw) = read_bounded(&path, READ_LIMIT) else {
        entry["health"] = json!("unavailable");
        return Some(entry);
    };
    let bytes = &raw[..raw.len().min(READ_LIMIT)];
    let Ok(text) = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes)) else {
        entry["health"] = json!("unavailable");
        return Some(entry);
    };
    let (clean, masked) = if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            let clean = super::details::redact_value(value.clone(), true, 0);
            if clean == value {
                (text.to_owned(), false)
            } else {
                (
                    serde_json::to_string_pretty(&clean).unwrap_or_default(),
                    true,
                )
            }
        } else {
            let clean = super::details::diagnostic_redact(text, 0);
            let masked = clean != text;
            (clean, masked)
        }
    } else {
        let clean = super::details::diagnostic_redact(text, 0);
        let masked = clean != text;
        (clean, masked)
    };
    entry["text"] = json!(clean);
    entry["masked"] = json!(masked);
    entry["editable"] = json!(
        entry["editable"] == true
            && !masked
            && raw.len() <= READ_LIMIT
            && text.chars().count() <= EDIT_LIMIT
    );
    entry["sha256"] = json!(format!("{:x}", Sha256::digest(&raw)));
    entry["truncated"] = json!(raw.len() > READ_LIMIT);
    let format = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let parser = if matches!(format.as_str(), "yaml" | "yml") {
        yaml_parser(table)
    } else {
        None
    };
    if matches!(format.as_str(), "yaml" | "yml") {
        entry["validation"] = json!(if parser.is_some() { "syntax" } else { "basic" });
    }
    let mut parsed = super::structured::parse_document(&clean, &format);
    if let Some((runtime, package)) = parser {
        if let Some(value) = parse_yaml(&clean, &runtime, &package) {
            parsed = Some((value, "YAML"));
        }
    }
    if let Some((value, kind)) = parsed {
        entry["parsed"] = super::details::redact_value(value, true, 0);
        entry["parsed_format"] = json!(kind);
        entry["parsed_truncated"] = json!(false);
    }
    Some(entry)
}

#[derive(Debug)]
pub(crate) enum WriteError {
    Invalid,
    Conflict,
    Io,
}

fn yaml_parser(table: &toml::map::Map<String, toml::Value>) -> Option<(PathBuf, PathBuf)> {
    let runtime = PathBuf::from(table.get("command")?.as_str()?);
    if !super::source_introduction::safe(&runtime)
        || !runtime.is_file()
        || !runtime.file_stem()?.to_str()?.eq_ignore_ascii_case("node")
    {
        return None;
    }
    for root in super::source_introduction::roots(table) {
        for parent in root.ancestors().take(5) {
            let package = parent.join("node_modules/js-yaml");
            if super::source_introduction::safe(&package) && package.join("package.json").is_file()
            {
                return Some((runtime, package));
            }
        }
    }
    None
}

fn validate_yaml(
    text: &str,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<(), WriteError> {
    if let Some((runtime, package)) = yaml_parser(table) {
        let package =
            serde_json::to_string(&package.to_string_lossy()).map_err(|_| WriteError::Invalid)?;
        let code=format!("try{{require({package}).load(require('fs').readFileSync(0,'utf8'));}}catch{{process.exit(1)}}");
        return node_execute(&runtime, &code, text, false).map(|_| ());
    }
    if text.lines().any(|line| {
        line.chars()
            .take_while(|ch| ch.is_whitespace())
            .any(|ch| ch == '\t')
    }) {
        return Err(WriteError::Invalid);
    }
    Ok(())
}

fn node_execute(
    runtime: &Path,
    code: &str,
    text: &str,
    capture: bool,
) -> Result<Vec<u8>, WriteError> {
    let mut input = tempfile::tempfile().map_err(|_| WriteError::Io)?;
    input
        .write_all(text.as_bytes())
        .and_then(|_| input.seek(SeekFrom::Start(0)).map(|_| ()))
        .map_err(|_| WriteError::Io)?;
    let mut output = tempfile::tempfile().map_err(|_| WriteError::Io)?;
    let stdout = if capture {
        Stdio::from(output.try_clone().map_err(|_| WriteError::Io)?)
    } else {
        Stdio::null()
    };
    let mut command = Command::new(runtime);
    command
        .args(["-e", code])
        .stdin(Stdio::from(input))
        .stdout(stdout)
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|_| WriteError::Invalid)?;
    let began = Instant::now();
    loop {
        if capture {
            match output.metadata() {
                Ok(meta) if meta.len() <= 1024 * 1024 => {}
                Ok(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(WriteError::Invalid);
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(WriteError::Io);
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err(WriteError::Invalid),
            Ok(None) if began.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(WriteError::Invalid);
            }
        }
    }
    let mut result = Vec::new();
    if capture {
        output
            .seek(SeekFrom::Start(0))
            .map_err(|_| WriteError::Io)?;
        output
            .take(1024 * 1024 + 1)
            .read_to_end(&mut result)
            .map_err(|_| WriteError::Io)?;
        if result.len() > 1024 * 1024 {
            return Err(WriteError::Invalid);
        }
    }
    Ok(result)
}

fn parse_yaml(text: &str, runtime: &Path, package: &Path) -> Option<Value> {
    let package = serde_json::to_string(&package.to_string_lossy()).ok()?;
    let code=format!("try{{let n=0;function p(v,d=0){{if(++n>200||d>8)return '[內容截斷]';if(typeof v==='string')return v;if(Array.isArray(v))return v.slice(0,100).map(x=>p(x,d+1));if(v&&typeof v==='object')return Object.fromEntries(Object.entries(v).slice(0,100).map(([k,x])=>[k.slice(0,160),p(x,d+1)]));return v}}process.stdout.write(JSON.stringify(p(require({package}).load(require('fs').readFileSync(0,'utf8')))))}}catch{{process.exit(1)}}");
    serde_json::from_slice(&node_execute(runtime, &code, text, true).ok()?).ok()
}

fn validate(
    path: &Path,
    text: &str,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<(), WriteError> {
    if text.chars().count() > EDIT_LIMIT || text.len() > READ_LIMIT || text.contains('\0') {
        return Err(WriteError::Invalid);
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "json" {
        let value: Value = serde_json::from_str(text).map_err(|_| WriteError::Invalid)?;
        if super::details::redact_value(value.clone(), true, 0) != value {
            return Err(WriteError::Invalid);
        }
    } else {
        if extension == "toml" {
            text.parse::<toml::Value>()
                .map_err(|_| WriteError::Invalid)?;
        }
        if matches!(extension.as_str(), "yaml" | "yml") {
            validate_yaml(text, table)?;
        }
        if super::details::diagnostic_redact(text, 0) != text {
            return Err(WriteError::Invalid);
        }
    }
    Ok(())
}

pub(crate) fn write_document(
    home: &Path,
    server: &str,
    id: &str,
    text: &str,
    expected: &str,
) -> Result<Value, WriteError> {
    let table = configuration(home, server).ok_or(WriteError::Invalid)?;
    let initial = read_document(&table, id).ok_or(WriteError::Invalid)?;
    if initial["editable"] != true {
        return Err(WriteError::Invalid);
    }
    if initial["sha256"].as_str() != Some(expected) {
        return Err(WriteError::Conflict);
    }
    let path = PathBuf::from(initial["path"].as_str().ok_or(WriteError::Invalid)?);
    validate(&path, text, &table)?;
    let metadata = safe_file(&path).ok_or(WriteError::Invalid)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".lam-")
        .suffix(".tmp")
        .tempfile_in(path.parent().ok_or(WriteError::Invalid)?)
        .map_err(|_| WriteError::Io)?;
    temporary
        .write_all(text.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| WriteError::Io)?;
    temporary
        .as_file()
        .set_permissions(metadata.permissions())
        .map_err(|_| WriteError::Io)?;
    let current = configuration(home, server).ok_or(WriteError::Conflict)?;
    if current != table
        || read_document(&current, id).is_none_or(|entry| {
            entry["editable"] != true || entry["sha256"].as_str() != Some(expected)
        })
        || safe_file(&path).is_none()
    {
        return Err(WriteError::Conflict);
    }
    temporary.persist(&path).map_err(|_| WriteError::Io)?;
    Ok(read_document(&current, id).unwrap_or(Value::Null))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn yaml_validation_uses_existing_parser_and_stops_timeout() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().canonicalize().unwrap();
        let package = root.join("node_modules/js-yaml");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("package.json"), "{}").unwrap();
        let runtime = root.join("node");
        fs::write(&runtime, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        let table = toml::map::Map::from_iter([
            (
                "command".into(),
                toml::Value::String(runtime.to_string_lossy().into_owned()),
            ),
            (
                "cwd".into(),
                toml::Value::String(root.to_string_lossy().into_owned()),
            ),
        ]);
        assert!(yaml_parser(&table).is_some());
        assert!(matches!(
            validate_yaml("enabled: true\n", &table),
            Err(WriteError::Invalid)
        ));
        fs::write(&runtime, "#!/bin/sh\nexec sleep 10\n").unwrap();
        let start = Instant::now();
        assert!(matches!(
            validate_yaml("enabled: true\n", &table),
            Err(WriteError::Invalid)
        ));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn yaml_preview_uses_configured_parser_and_masks_nested_output() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().canonicalize().unwrap();
        let package = root.join("node_modules/js-yaml");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("package.json"), "{}").unwrap();
        let runtime = root.join("node");
        fs::write(
            &runtime,
            "#!/bin/sh\nprintf '{\"nested\":{\"password\":\"PRIVATE\"}}'\n",
        )
        .unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        let file = root.join("settings.yaml");
        fs::write(&file, "name: &anchor fixture\ncopy: *anchor\n").unwrap();
        let table = toml::map::Map::from_iter([
            (
                "command".into(),
                toml::Value::String(runtime.to_string_lossy().into_owned()),
            ),
            (
                "cwd".into(),
                toml::Value::String(root.to_string_lossy().into_owned()),
            ),
            (
                "args".into(),
                toml::Value::Array(vec![toml::Value::String(
                    file.to_string_lossy().into_owned(),
                )]),
            ),
        ]);
        let id = documents(&table)
            .into_iter()
            .find(|entry| entry["name"] == "settings.yaml")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let result = read_document(&table, &id).unwrap();
        assert_eq!(result["validation"], "syntax");
        assert_eq!(result["parsed_format"], "YAML");
        assert_eq!(result["parsed"]["nested"]["password"], "[已隱藏]");
        assert!(!result.to_string().contains("PRIVATE"));
    }

    #[test]
    fn yaml_preview_script_runs_with_node_and_bounded_projection() {
        if Command::new("node")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return;
        }
        let folder = tempfile::tempdir().unwrap();
        let package = folder.path().join("module");
        fs::create_dir(&package).unwrap();
        fs::write(
            package.join("index.js"),
            "exports.load = text => ({value: text.trim(), nested: {ok: true}});",
        )
        .unwrap();
        let value = parse_yaml("fixture\n", Path::new("node"), &package).unwrap();
        assert_eq!(value["value"], "fixture");
        assert_eq!(value["nested"]["ok"], true);
    }

    #[test]
    fn yaml_preview_rejects_oversized_parser_output() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = folder.path().join("node");
        fs::write(&runtime, "#!/bin/sh\nexec head -c 1048577 /dev/zero\n").unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            node_execute(&runtime, "ignored", "", true),
            Err(WriteError::Invalid)
        ));
    }
}
