use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

fn safe(relative: &Path) -> bool {
    relative.components().all(|part| match part {
        Component::Normal(name) => !name.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

fn describe(root: &Path, path: &Path) -> Option<Value> {
    let relative = path.strip_prefix(root).ok()?;
    if !safe(relative) {
        return None;
    }
    let metadata = fs::symlink_metadata(path).ok()?;
    if super::redirected(&metadata) || !metadata.is_file() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();
    let suffix = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ["auth.json", "credentials.json", "secrets.json"].contains(&lower.as_str())
        || ["pem", "key", "pfx", "p12"].contains(&suffix.as_str())
    {
        return None;
    }
    let readable = ["md", "markdown", "mdown", "txt", "rst", "adoc", "asciidoc"]
        .contains(&suffix.as_str())
        || ["readme", "license", "changelog"].contains(&lower.as_str());
    let modified = metadata
        .modified()
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map(|t| t.to_rfc3339());
    Some(
        json!({"name":name,"relative_path":relative.to_string_lossy().replace('\\',"/"),"path":canonical.to_string_lossy(),"kind":if readable {"document"} else if ["py","pyw"].contains(&suffix.as_str()) {"python"} else {"file"},"format":suffix,"readable":readable,"bytes":metadata.len(),"modified_at":modified}),
    )
}

pub(crate) fn render(
    skill: &str,
    candidate: &lam_session_core::SkillRead,
    selected: Option<&str>,
) -> Option<Value> {
    if candidate.path.starts_with("//") || candidate.path.starts_with("\\\\") {
        return None;
    }
    let raw = if let Some(rest) = candidate.path.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME")?).join(rest)
    } else {
        PathBuf::from(&candidate.path)
    };
    let path = if raw.is_absolute() {
        raw
    } else {
        let base = Path::new(candidate.workdir.as_deref()?);
        if !base.is_absolute() {
            return None;
        }
        base.join(raw)
    };
    let path = path.canonicalize().ok()?;
    if !path.file_name()?.to_str()?.eq_ignore_ascii_case("skill.md") || !path.is_file() {
        return None;
    }
    let root = path.parent()?.canonicalize().ok()?;
    let mut files = BTreeMap::new();
    for primary in [path.clone(), root.join("README.md")] {
        if let Some(value) = describe(&root, &primary) {
            files.insert(value["relative_path"].as_str()?.to_owned(), value);
        }
    }
    let mut truncated = false;
    let walker = WalkDir::new(&root)
        .max_depth(5)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.path() == root
                || (!entry.file_name().to_string_lossy().starts_with('.')
                    && !["__pycache__", "node_modules", "venv", "dist", "build"]
                        .contains(&entry.file_name().to_string_lossy().as_ref()))
        });
    for (visited, entry) in walker.enumerate() {
        if visited >= 1000 || files.len() >= 200 {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        if entry.depth() == 5 && entry.file_type().is_dir() {
            truncated = true;
        }
        if let Some(value) = describe(&root, entry.path()) {
            files.insert(value["relative_path"].as_str()?.to_owned(), value);
        }
    }
    let mut inventory: Vec<_> = files.into_values().collect();
    inventory.sort_by_key(|entry| {
        let path = entry["relative_path"]
            .as_str()
            .unwrap_or("")
            .to_ascii_lowercase();
        (!["skill.md", "readme.md"].contains(&path.as_str()), path)
    });
    let mut documents = Vec::new();
    for entry in &inventory {
        let relative = entry["relative_path"].as_str()?;
        if entry["readable"] != true
            || !selected.map_or_else(
                || ["skill.md", "readme.md"].contains(&relative.to_ascii_lowercase().as_str()),
                |name| name == relative,
            )
        {
            continue;
        }
        let path = root.join(relative);
        let Ok(before) = fs::symlink_metadata(&path) else {
            continue;
        };
        if super::redirected(&before) {
            continue;
        }
        let Ok(mut stream) = super::open_source(&root, &path) else {
            continue;
        };
        let Ok(actual) = stream.metadata() else {
            continue;
        };
        if super::file_identity(&before) != super::file_identity(&actual) || !actual.is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        if stream
            .by_ref()
            .take(262145)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        let clipped = &bytes[..bytes.len().min(262144)];
        let text =
            String::from_utf8_lossy(clipped.strip_prefix(b"\xef\xbb\xbf").unwrap_or(clipped));
        let mut value = entry.clone();
        value["text"] = json!(super::details::redact_document(&text));
        value["read_bytes"] = json!(clipped.len());
        value["truncated"] = json!(bytes.len() > clipped.len());
        value["sha256"] = json!(format!("{:x}", Sha256::digest(clipped)));
        documents.push(value);
    }
    Some(
        json!({"skill":skill,"root":root.to_string_lossy(),"files":inventory,"files_truncated":truncated,"documents":documents}),
    )
}
