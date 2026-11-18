use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const READ_LIMIT: usize = 64 * 1024;
const TOTAL_LIMIT: usize = 256 * 1024;
const DOCUMENT_LIMIT: usize = 64;
// Retain existing boundaries and add only non-letter/number Rust word prefixes.
// Preserve the captured prefix; Unicode version differences remain separate.
static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\b|(?P<prefix>[\w&&[^\p{L}\p{N}_]]))(?:Bearer[\s\x1c-\x1f]+[A-Za-z0-9._~+/-]+=*|sk-[A-Za-z0-9_-]{12,})")
        .expect("token pattern")
});
static ASSIGNMENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:\b|(?P<prefix>[\w&&[^\p{L}\p{N}_]]))(?:api[_-]?key|password|secret|authorization|access[_-]?token|token)["']?[\s\x1c-\x1f]*[:=][\s\x1c-\x1f]*(?:"[^"]*"|'[^']*'|[^\s\x1c-\x1f,;]+)"#).expect("assignment pattern")
});

fn safe(path: &Path) -> io::Result<bool> {
    let display = path.to_string_lossy();
    if !path.is_absolute()
        || display.starts_with("//")
        || display.starts_with("\\\\")
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Ok(false);
    }
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(meta) if super::redirected(&meta) => return Ok(false),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn bounded_file(path: &Path) -> Option<Vec<u8>> {
    if !safe(path).ok()? {
        return None;
    }
    let before = fs::symlink_metadata(path).ok()?;
    if !before.is_file() || before.len() > READ_LIMIT as u64 {
        return None;
    }
    let root = path.ancestors().last()?;
    let mut file = super::open_source(root, path).ok()?;
    let after = file.metadata().ok()?;
    if !after.is_file()
        || super::redirected(&after)
        || super::file_identity(&before) != super::file_identity(&after)
        || after.len() > READ_LIMIT as u64
    {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((READ_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= READ_LIMIT).then_some(bytes)
}

fn git(args: &[&std::ffi::OsStr], input: Option<Vec<u8>>) -> Option<Vec<u8>> {
    let mut child = Command::new("git")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let result = stdout.take((READ_LIMIT + 1) as u64).read_to_end(&mut out);
        result.ok().map(|_| out)
    });
    let writer = input.map(|input| {
        let stdin = child.stdin.take();
        std::thread::spawn(move || stdin.and_then(|mut stdin| stdin.write_all(&input).ok()))
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let wrote = writer.is_none_or(|writer| writer.join().ok().flatten().is_some());
    let output = reader.join().ok()??;
    (wrote && status.is_some_and(|status| status.success()) && output.len() <= READ_LIMIT)
        .then_some(output)
}

fn repository_id(value: &str) -> String {
    let value = ["https://", "http://", "ssh://git@", "git@"]
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .unwrap_or(value);
    let value = value.replace(':', "/");
    value
        .strip_suffix(".git")
        .unwrap_or(&value)
        .trim_end_matches('/')
        .to_owned()
}

fn workspace_sources(root: &Path) -> Vec<PathBuf> {
    let Some(parent) = root.parent() else {
        return Vec::new();
    };
    let Some(name) = root.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let Some(modules) = bounded_file(&parent.join(".gitmodules")) else {
        return Vec::new();
    };
    let args = [
        "config".as_ref(),
        "--file".as_ref(),
        "-".as_ref(),
        "--null".as_ref(),
        "--get-regexp".as_ref(),
        r"^.*\.(path|url)$".as_ref(),
    ];
    let Some(output) = git(&args, Some(modules)) else {
        return Vec::new();
    };
    let mut sections: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Ok(record) = std::str::from_utf8(record) else {
            return Vec::new();
        };
        let Some((key, value)) = record.split_once('\n') else {
            return Vec::new();
        };
        let Some((section, field)) = key.rsplit_once('.') else {
            return Vec::new();
        };
        let index = sections
            .iter()
            .position(|(name, _, _)| name == section)
            .unwrap_or_else(|| {
                sections.push((section.to_owned(), None, None));
                sections.len() - 1
            });
        let slot = if field == "path" {
            &mut sections[index].1
        } else if field == "url" {
            &mut sections[index].2
        } else {
            continue;
        };
        if slot.is_some() {
            return Vec::new();
        }
        *slot = Some(value.to_owned());
    }
    let Some((_, _, Some(url))) = sections
        .iter()
        .find(|(_, path, _)| path.as_deref() == Some(name))
    else {
        return Vec::new();
    };
    let args = [
        "-C".as_ref(),
        root.as_os_str(),
        "config".as_ref(),
        "--get".as_ref(),
        "remote.origin.url".as_ref(),
    ];
    let Some(origin) = git(&args, None).and_then(|output| String::from_utf8(output).ok()) else {
        return Vec::new();
    };
    if repository_id(origin.trim()) != repository_id(url) {
        return Vec::new();
    }
    for (registry, prefix) in [
        ("workspace.json", "project-agents/"),
        ("governance/projects.json", "governance/projects/"),
    ] {
        let Some(bytes) = bounded_file(&parent.join(registry)) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return Vec::new();
        };
        let Some(projects) = value.get("projects").and_then(Value::as_array) else {
            return Vec::new();
        };
        let Some(project) = projects.iter().find(|entry| entry["path"] == name) else {
            return Vec::new();
        };
        if project["repository"]
            .as_str()
            .is_none_or(|repository| repository_id(repository) != repository_id(url))
        {
            return Vec::new();
        }
        let Some(source) = project["instructions"].as_str() else {
            return Vec::new();
        };
        if !source.starts_with(prefix)
            || !source.ends_with("/AGENTS.md")
            || source.contains(['\\', ':'])
            || source
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Vec::new();
        }
        let mut paths = vec![parent.join("AGENTS.md"), parent.join(source)];
        if project["entry"] == "AGENTS.override.md" {
            paths.push(root.join("AGENTS.md"));
        }
        return paths;
    }
    let entry = parent.join(".codex/agent-guidance/game-entry.md");
    let game = parent.join(".codex/agent-guidance/game-AGENTS.md");
    let active = root.join("AGENTS.md");
    if bounded_file(&game).is_some()
        && bounded_file(&entry)
            .zip(bounded_file(&active))
            .is_some_and(|(a, b)| a == b)
    {
        return vec![parent.join("AGENTS.md"), game];
    }
    Vec::new()
}

fn read_file(root: &Path, path: &Path, remaining: &mut usize) -> Option<Value> {
    let filename = path.file_name()?.to_string_lossy().into_owned();
    let mut item = json!({"file":path.to_string_lossy(),"filename":filename,"root":root.to_string_lossy(),"text":null,"truncated":false,"read_bytes":0,"modified_at":null,"health":"missing"});
    match safe(path) {
        Ok(false) => item["health"] = json!("rejected"),
        Err(_) => item["health"] = json!("unavailable"),
        Ok(true) => {
            let before = match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() => meta,
                Ok(_) => return None,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
                Err(_) => {
                    item["health"] = json!("unavailable");
                    return Some(item);
                }
            };
            let root_of_path = path.ancestors().last().unwrap_or(path);
            match super::open_source(root_of_path, path) {
                Ok(mut file) => {
                    let Ok(meta) = file.metadata() else {
                        item["health"] = json!("unavailable");
                        return Some(item);
                    };
                    if !meta.is_file()
                        || super::redirected(&meta)
                        || super::file_identity(&meta) != super::file_identity(&before)
                    {
                        item["health"] = json!("rejected");
                        return Some(item);
                    }
                    let mut bytes = Vec::new();
                    if (&mut file)
                        .take(READ_LIMIT.min(*remaining) as u64)
                        .read_to_end(&mut bytes)
                        .is_err()
                    {
                        item["health"] = json!("unavailable");
                        return Some(item);
                    }
                    *remaining -= bytes.len();
                    let original = String::from_utf8_lossy(&bytes);
                    let first = TOKENS.replace_all(&original, "${prefix}[已隱藏]");
                    let redacted = ASSIGNMENTS.replace_all(&first, "${prefix}[已隱藏]").into_owned();
                    let modified = meta.modified().ok().map(|time| {
                        DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true)
                    });
                    item["text"] = json!(redacted);
                    item["truncated"] = json!(meta.len() > bytes.len() as u64);
                    item["read_bytes"] = json!(bytes.len());
                    item["modified_at"] = json!(modified);
                    item["health"] = json!("ok");
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
                Err(_) => item["health"] = json!("unavailable"),
            }
        }
    }
    Some(item)
}

pub(crate) fn read(roots: &[PathBuf]) -> Value {
    let mut seen = BTreeSet::new();
    let mut documents = Vec::new();
    let mut remaining = TOTAL_LIMIT;
    for root in roots.iter().take(32) {
        let override_path = root.join("AGENTS.override.md");
        let use_override = safe(&override_path).unwrap_or(false)
            && fs::symlink_metadata(&override_path)
                .is_ok_and(|meta| meta.is_file() && meta.len() > 0);
        let path = root.join(if use_override {
            "AGENTS.override.md"
        } else {
            "AGENTS.md"
        });
        let mut paths = if safe(root).unwrap_or(false) {
            workspace_sources(root)
        } else {
            Vec::new()
        };
        paths.push(path);
        for path in paths {
            if documents.len() >= DOCUMENT_LIMIT || remaining == 0 || !seen.insert(path.clone()) {
                continue;
            }
            if let Some(item) = read_file(root, &path, &mut remaining) {
                documents.push(item);
            }
        }
    }
    let health = if documents.is_empty() {
        "missing"
    } else if documents.iter().all(|item| item["health"] == "ok") {
        "ok"
    } else {
        "partly_unavailable"
    };
    json!({"documents":documents,"health":health,"read_limit":READ_LIMIT,"scope":"selected_root_instruction_files"})
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn instruction_root_limit_counts_selected_roots_before_deduplication() {
        let home = tempfile::tempdir().unwrap();
        let base = home.path().canonicalize().unwrap();
        let roots: Vec<_> = (0..33).map(|index| base.join(index.to_string())).collect();
        for (index, root) in roots.iter().enumerate() {
            fs::create_dir(root).unwrap();
            fs::write(root.join("AGENTS.md"), format!("root {index}")).unwrap();
        }
        let result = read(&roots);
        let documents = result["documents"].as_array().unwrap();
        assert_eq!(documents.len(), 32);
        assert_eq!(documents[31]["text"], "root 31");
        assert_eq!(result["health"], "ok");
        let repeated: Vec<_> = std::iter::once(roots[0].clone()).chain(roots).collect();
        let result = read(&repeated);
        let documents = result["documents"].as_array().unwrap();
        assert_eq!(documents.len(), 31);
        assert_eq!(documents[30]["text"], "root 30");
    }

    #[test]
    fn instruction_document_limit_preserves_registered_source_order() {
        let home = tempfile::tempdir().unwrap();
        let base = home.path().canonicalize().unwrap();
        let mut roots = Vec::new();
        for index in 0..22 {
            let parent = base.join(index.to_string());
            let child = parent.join("child");
            fs::create_dir_all(&child).unwrap();
            assert!(Command::new("git").args(["init", "-q"]).arg(&child).status().unwrap().success());
            assert!(Command::new("git").arg("-C").arg(&child)
                .args(["remote", "add", "origin", "https://example.invalid/owner/child.git"])
                .status().unwrap().success());
            fs::write(parent.join(".gitmodules"), "[submodule \"child\"]\npath = child\nurl = https://example.invalid/owner/child.git\n").unwrap();
            fs::create_dir_all(parent.join("project-agents/child")).unwrap();
            fs::write(parent.join("workspace.json"), json!({"projects":[{
                "path":"child","repository":"https://example.invalid/owner/child.git",
                "instructions":"project-agents/child/AGENTS.md"
            }]}).to_string()).unwrap();
            fs::write(parent.join("AGENTS.md"), format!("parent {index}")).unwrap();
            fs::write(parent.join("project-agents/child/AGENTS.md"), format!("canonical {index}")).unwrap();
            fs::write(child.join("AGENTS.md"), format!("entry {index}")).unwrap();
            roots.push(child);
        }
        let result = read(&roots);
        let documents = result["documents"].as_array().unwrap();
        assert_eq!(documents.len(), 64);
        for index in 0..21 {
            assert_eq!(documents[index * 3]["text"], format!("parent {index}"));
            assert_eq!(documents[index * 3 + 1]["text"], format!("canonical {index}"));
            assert_eq!(documents[index * 3 + 2]["text"], format!("entry {index}"));
        }
        assert_eq!(documents[63]["text"], "parent 21");
        assert_eq!(result["health"], "ok");
        assert_eq!(fs::read_to_string(roots[21].join("AGENTS.md")).unwrap(), "entry 21");
    }

    #[test]
    fn instruction_masking_adds_nonletter_boundaries_without_writes() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("AGENTS.md");
        for prefix in ['\u{301}', '\u{200c}', '\u{200d}', '\u{203f}'] {
            let source = format!("{prefix}Bearer SYNTHETIC_TOKEN\n{prefix}sk-SYNTHETIC_TOKEN\n{prefix}api_key=one,{prefix}token='two'; public\n中Bearer public\nordinary text\n²Bearer SYNTHETIC_TOKEN");
            fs::write(&path, &source).unwrap();
            let value = read(&[root.clone()]);
            assert_eq!(value["documents"][0]["health"], "ok");
            assert_eq!(value["documents"][0]["text"], format!("{prefix}[已隱藏]\n{prefix}[已隱藏]\n{prefix}[已隱藏],{prefix}[已隱藏]; public\n中Bearer public\nordinary text\n²[已隱藏]"));
            assert_eq!(value["documents"][0]["read_bytes"], source.len());
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }

    #[test]
    fn instruction_masking_recognizes_python_control_whitespace_without_writes() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("AGENTS.md");
        for separator in ['\u{1c}', '\u{1d}', '\u{1e}', '\u{1f}'] {
            let source = format!("Bearer{separator}SYNTHETIC_TOKEN\napi_key{separator}={separator}SYNTHETIC_VALUE\nordinary text");
            fs::write(&path, &source).unwrap();
            let value = read(&[root.clone()]);
            assert_eq!(value["documents"][0]["health"], "ok");
            assert_eq!(
                value["documents"][0]["text"],
                "[已隱藏]\n[已隱藏]\nordinary text"
            );
            assert_eq!(value["documents"][0]["read_bytes"], source.len());
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }

    #[test]
    fn rejects_redirected_root_without_reading_instruction_body() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("real");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("AGENTS.md"), "PRIVATE_CONTENT").unwrap();
        let link = home.path().join("link");
        std::os::unix::fs::symlink(root, &link).unwrap();
        let result = read(&[link]);
        assert_eq!(result["documents"][0]["health"], "rejected");
        assert!(result["documents"][0]["text"].is_null());
        assert!(!result.to_string().contains("PRIVATE_CONTENT"));
    }

    #[test]
    fn registered_workspace_requires_matching_repository_and_safe_source() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().canonicalize().unwrap();
        let child = parent.join("child");
        fs::create_dir(&child).unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .arg(&child)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .arg("-C")
            .arg(&child)
            .args([
                "remote",
                "add",
                "origin",
                "https://example.invalid/owner/child.git"
            ])
            .status()
            .unwrap()
            .success());
        fs::write(
            parent.join(".gitmodules"),
            "[submodule \"child\"]\npath = child\nurl = https://example.invalid/owner/child.git\n",
        )
        .unwrap();
        fs::write(parent.join("AGENTS.md"), "Parent").unwrap();
        fs::write(child.join("AGENTS.md"), "Entry").unwrap();
        fs::create_dir_all(parent.join("project-agents/child")).unwrap();
        fs::write(parent.join("project-agents/child/AGENTS.md"), "Canonical").unwrap();
        let registry = |repository: &str, source: &str, entry: &str| {
            json!({"projects":[{"path":"child","repository":repository,"instructions":source,"entry":entry}]}).to_string()
        };
        let file = parent.join("workspace.json");
        fs::write(
            &file,
            registry(
                "https://example.invalid/owner/child.git",
                "project-agents/child/AGENTS.md",
                "",
            ),
        )
        .unwrap();
        let result = read(std::slice::from_ref(&child));
        assert_eq!(
            result["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|document| document["text"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["Parent", "Canonical", "Entry"]
        );
        fs::write(
            &file,
            registry(
                "https://example.invalid/owner/other.git",
                "project-agents/child/AGENTS.md",
                "",
            ),
        )
        .unwrap();
        assert_eq!(
            read(std::slice::from_ref(&child))["documents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        fs::write(
            &file,
            registry(
                "https://example.invalid/owner/child.git",
                "project-agents/../../outside/AGENTS.md",
                "",
            ),
        )
        .unwrap();
        assert_eq!(
            read(std::slice::from_ref(&child))["documents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        fs::write(
            &file,
            registry(
                "https://example.invalid/owner/child.git",
                "project-agents/child/AGENTS.md",
                "AGENTS.override.md",
            ),
        )
        .unwrap();
        fs::write(child.join("AGENTS.override.md"), "Override").unwrap();
        let result = read(std::slice::from_ref(&child));
        assert_eq!(
            result["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|document| document["text"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["Parent", "Canonical", "Entry", "Override"]
        );
        fs::remove_file(child.join("AGENTS.override.md")).unwrap();
        fs::remove_file(&file).unwrap();
        fs::create_dir_all(parent.join("governance/projects/child")).unwrap();
        fs::write(
            parent.join("governance/projects/child/AGENTS.md"),
            "Private source",
        )
        .unwrap();
        let governance = parent.join("governance/projects.json");
        fs::write(
            &governance,
            registry(
                "https://example.invalid/owner/child.git",
                "governance/projects/child/AGENTS.md",
                "",
            ),
        )
        .unwrap();
        let result = read(std::slice::from_ref(&child));
        assert_eq!(result["documents"][1]["text"], "Private source");
        fs::remove_file(governance).unwrap();
        fs::create_dir_all(parent.join(".codex/agent-guidance")).unwrap();
        fs::write(parent.join(".codex/agent-guidance/game-entry.md"), "Entry").unwrap();
        fs::write(
            parent.join(".codex/agent-guidance/game-AGENTS.md"),
            "Game source",
        )
        .unwrap();
        let result = read(std::slice::from_ref(&child));
        assert_eq!(result["documents"][1]["text"], "Game source");
        fs::write(child.join("AGENTS.md"), "Different").unwrap();
        assert_eq!(read(&[child])["documents"].as_array().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod read_parity_tests {
    use super::*;

    #[test]
    fn instruction_read_budget_and_override_match_source() {
        let home = tempfile::tempdir().unwrap();
        let roots: Vec<_> = (0..5).map(|index| home.path().join(index.to_string())).collect();
        let body = format!("{}中", "x".repeat(READ_LIMIT - 1));
        for root in &roots {
            fs::create_dir(root).unwrap();
            fs::write(root.join("AGENTS.md"), &body).unwrap();
        }
        let result = read(&roots);
        let documents = result["documents"].as_array().unwrap();
        assert_eq!(documents.len(), 4);
        let expected = format!("{}\u{fffd}", "x".repeat(READ_LIMIT - 1));
        for document in documents {
            assert_eq!(document["read_bytes"], READ_LIMIT);
            assert_eq!(document["truncated"], true);
            assert_eq!(document["text"], expected);
        }
        fs::write(roots[0].join("AGENTS.md"), "base").unwrap();
        fs::write(roots[0].join("AGENTS.override.md"), "").unwrap();
        let fallback = read(&roots[..1]);
        assert_eq!(fallback["documents"][0]["filename"], "AGENTS.md");
        assert_eq!(fallback["documents"][0]["text"], "base");
        fs::write(roots[0].join("AGENTS.override.md"), "override").unwrap();
        let selected = read(&[roots[0].clone(), roots[0].clone()]);
        assert_eq!(selected["documents"].as_array().unwrap().len(), 1);
        assert_eq!(selected["documents"][0]["filename"], "AGENTS.override.md");
        assert_eq!(selected["documents"][0]["text"], "override");
    }
}
