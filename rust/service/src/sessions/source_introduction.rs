//! Read bounded local package introductions associated with configured launchers.
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Description {
    pub text: String,
    pub source: String,
    pub path: Option<String>,
}

static PRIVATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:auth|credential|secret|token|password|private[-_]?key)|^\.env(?:\.|$)")
        .unwrap()
});
static CODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```.*?```|~~~.*?~~~|<!--.*?-->").unwrap());
static FRONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)^---\n.*?\n---\n").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap());
static PARAGRAPH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").unwrap());
static MODULE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*$").unwrap());
static CACHE: LazyLock<Mutex<BTreeMap<String, (Instant, Option<Description>)>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

pub(super) fn safe(path: &Path) -> bool {
    let text = path.to_string_lossy();
    if !path.is_absolute()
        || text.starts_with("//")
        || text.starts_with("\\\\")
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| PRIVATE.is_match(name))
    {
        return false;
    }
    path.ancestors()
        .all(|part| match fs::symlink_metadata(part) {
            Ok(meta) => !super::redirected(&meta),
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        })
}

pub(super) fn roots(table: &toml::map::Map<String, toml::Value>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(cwd) = table
        .get("cwd")
        .and_then(toml::Value::as_str)
        .filter(|value| value.chars().count() < 512)
    {
        let path = PathBuf::from(cwd);
        if safe(&path) && path.is_dir() {
            roots.push(path);
        }
    }
    let mut candidates: Vec<&str> = table
        .get("args")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .take(32)
        .filter_map(toml::Value::as_str)
        .collect();
    if let Some(command) = table.get("command").and_then(toml::Value::as_str) {
        candidates.push(command);
    }
    for item in candidates {
        if item.chars().count() > 512 {
            continue;
        }
        let path = Path::new(item);
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !safe(path)
            || !path.is_file()
            || !["py", "js", "mjs", "cjs", "exe"].contains(&ext.as_str())
            || ["node", "python", "python3", "pythonw", "codex", "textlint"]
                .contains(&stem.as_str())
        {
            continue;
        }
        let Some(mut directory) = path.parent() else {
            continue;
        };
        for _ in 0..5 {
            roots.push(directory.to_path_buf());
            let name = directory
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if directory.join("package.json").is_file()
                || directory.join("pyproject.toml").is_file()
                || ["node_modules", "site-packages"].contains(&name)
                || !["src", "scripts", "bin", "build", "dist", "server", "lib"]
                    .contains(&name.to_ascii_lowercase().as_str())
            {
                break;
            }
            let Some(parent) = directory.parent() else {
                break;
            };
            if parent == directory {
                break;
            }
            directory = parent;
        }
    }
    let args = table.get("args").and_then(toml::Value::as_array);
    if let (Some(command), Some(args)) = (table.get("command").and_then(toml::Value::as_str), args)
    {
        let module = args
            .iter()
            .take(32)
            .position(|arg| arg.as_str() == Some("-m"))
            .and_then(|index| (index + 1 < 32).then(|| args.get(index + 1)).flatten())
            .and_then(toml::Value::as_str)
            .filter(|name| MODULE.is_match(name));
        let runtime = Path::new(command);
        if let (Some(module), true, Some(environment)) = (
            module,
            safe(runtime) && runtime.is_file(),
            runtime.parent().and_then(Path::parent),
        ) {
            let mut environments = vec![environment.join("Lib/site-packages")];
            if let Ok(entries) = fs::read_dir(environment.join("lib")) {
                environments.extend(
                    entries
                        .flatten()
                        .filter(|entry| entry.file_name().to_string_lossy().starts_with("python"))
                        .take(8)
                        .map(|entry| entry.path().join("site-packages")),
                );
            }
            for environment in environments {
                let mut location = module
                    .split('.')
                    .fold(environment, |path, part| path.join(part));
                if location.with_extension("py").is_file() {
                    if let Some(parent) = location.parent() {
                        location = parent.to_path_buf();
                    }
                }
                if location.is_dir() && safe(&location) {
                    roots.push(location.clone());
                    if let Some(parent) = location.parent() {
                        roots.push(parent.to_path_buf());
                    }
                }
            }
        }
    }
    let mut unique = Vec::new();
    for root in roots {
        if !unique.contains(&root) {
            unique.push(root);
            if unique.len() == 16 {
                break;
            }
        }
    }
    unique
}

fn read(path: &Path, exact_limit: bool) -> Option<String> {
    if !safe(path) {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(if exact_limit { 65537 } else { 65536 })
        .read_to_end(&mut bytes)
        .ok()?;
    if exact_limit && bytes.len() > 65536 {
        return None;
    }
    String::from_utf8(
        bytes
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&bytes)
            .to_vec(),
    )
    .ok()
}

fn result(text: &str, source: &str, path: &Path) -> Description {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Description {
        text: super::details::redact_description(&normalized.chars().take(400).collect::<String>()),
        source: source.into(),
        path: Some(path.to_string_lossy().into_owned()),
    }
}

fn extract(table: &toml::map::Map<String, toml::Value>) -> Option<Description> {
    let roots = roots(table);
    for root in &roots {
        for name in ["package.json", "pyproject.toml"] {
            let path = root.join(name);
            let Some(body) = read(&path, true) else {
                continue;
            };
            let text = if name == "package.json" {
                serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
            } else {
                body.parse::<toml::Value>().ok().and_then(|value| {
                    value
                        .get("project")
                        .and_then(|project| project.get("description"))
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned)
                })
            };
            if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                return Some(result(&text, name, &path));
            }
        }
    }
    if let Some(module) = table
        .get("args")
        .and_then(toml::Value::as_array)
        .and_then(|args| {
            args.iter()
                .position(|arg| arg.as_str() == Some("-m"))
                .and_then(|index| args.get(index + 1))
        })
        .and_then(toml::Value::as_str)
    {
        let filename = format!("{}.py", module.rsplit('.').next().unwrap_or(""));
        for root in &roots {
            let path = root.join(&filename);
            let Some(body) = read(&path, true) else {
                continue;
            };
            if let Some(text) = lam_session_core::python_module_docstring(&body) {
                return Some(result(&text, &filename, &path));
            }
        }
    }
    for root in &roots {
        for name in ["README.md", "readme.md"] {
            let path = root.join(name);
            let Some(body) = read(&path, false) else {
                continue;
            };
            if body.lines().any(|line| line.trim_end() == "# Node.js") {
                continue;
            }
            let body = FRONT.replace(&body, "");
            let body = CODE.replace_all(&body, "");
            for paragraph in PARAGRAPH.split(&body) {
                let paragraph = paragraph.trim();
                if paragraph.is_empty()
                    || ["#", "!", "[![", "|", "- ", "* ", ">", "<"]
                        .iter()
                        .any(|prefix| paragraph.starts_with(prefix))
                    || paragraph.contains(" · ") && paragraph.chars().count() < 100
                {
                    continue;
                }
                let text = LINK.replace_all(paragraph, "$1");
                if text.starts_with("Node.js is an open-source")
                    || text.starts_with("For information on using Node.js")
                {
                    continue;
                }
                return Some(result(&text, name, &path));
            }
        }
    }
    None
}

pub(super) fn introduction(table: &toml::map::Map<String, toml::Value>) -> Option<Description> {
    let key = format!(
        "{:x}",
        Sha256::digest(toml::Value::Table(table.clone()).to_string().as_bytes())
    );
    if let Some((time, value)) = CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
    {
        if time.elapsed() < Duration::from_secs(60) {
            return value.clone();
        }
    }
    let result = extract(table);
    let mut cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(key, (Instant::now(), result.clone()));
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn local_introduction_rejects_symlink_and_private_names() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let readme = root.join("README.md");
        fs::write(&readme, "Public introduction").unwrap();
        let link = root.join("link.md");
        std::os::unix::fs::symlink(&readme, &link).unwrap();
        assert!(read(&link, false).is_none());
        assert!(!safe(&root.join("credentials.json")));
        assert_eq!(read(&readme, false).as_deref(), Some("Public introduction"));
    }
}
