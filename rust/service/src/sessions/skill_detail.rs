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
        .to_lowercase();
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
    if path.file_name()?.to_str()?.to_lowercase() != "skill.md" || !path.is_file() {
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
    let mut walker = WalkDir::new(&root)
        .max_depth(5)
        .follow_links(false)
        .into_iter();
    let mut visited = 0;
    while let Some(entry) = walker.next() {
        if entry.as_ref().is_ok_and(|entry| entry.depth() == 0) {
            continue;
        }
        visited += 1;
        if visited > 1000 || files.len() >= 200 {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        if entry.file_name().to_string_lossy().starts_with('.')
            || ["__pycache__", "node_modules", "venv", "dist", "build"]
                .contains(&entry.file_name().to_string_lossy().as_ref())
        {
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
            continue;
        }
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
            .to_lowercase();
        (!["skill.md", "readme.md"].contains(&path.as_str()), path)
    });
    let mut documents = Vec::new();
    for entry in &inventory {
        let relative = entry["relative_path"].as_str()?;
        if entry["readable"] != true
            || !selected.map_or_else(
                || ["skill.md", "readme.md"].contains(&relative.to_lowercase().as_str()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_visit_budget_counts_hidden_entries() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("SKILL.md");
        fs::write(&path, "primary fixture").unwrap();
        let candidate = lam_session_core::SkillRead {
            name: "fixture".into(), path: path.to_string_lossy().into_owned(), workdir: None,
        };
        for index in 0..999 {
            fs::write(root.join(format!(".hidden-{index:04}")), "excluded fixture").unwrap();
        }
        for truncated in [false, true] {
            if truncated {
                fs::write(root.join(".hidden-extra"), "excluded fixture").unwrap();
            }
            let detail = render("fixture", &candidate, None).unwrap();
            assert_eq!(detail["files_truncated"], truncated);
            assert_eq!(detail["files"].as_array().unwrap().len(), 1);
            assert_eq!(detail["documents"].as_array().unwrap().len(), 1);
            assert_eq!(detail["documents"][0]["text"], "primary fixture");
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1000 + usize::from(truncated));
        }
    }

    #[test]
    fn inventory_skips_excluded_directory_contents() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("SKILL.md");
        fs::write(&path, "primary fixture").unwrap();
        let candidate = lam_session_core::SkillRead {
            name: "fixture".into(), path: path.to_string_lossy().into_owned(), workdir: None,
        };
        for directory in [".hidden", "__pycache__", "node_modules", "venv", "dist", "build"] {
            fs::create_dir(root.join(directory)).unwrap();
            fs::write(root.join(directory).join("guide.md"), "excluded fixture").unwrap();
            let detail = render("fixture", &candidate, Some(&format!("{directory}/guide.md"))).unwrap();
            assert_eq!(detail["files"].as_array().unwrap().len(), 1);
            assert_eq!(detail["files_truncated"], false);
            assert!(detail["documents"].as_array().unwrap().is_empty());
        }
    }

    #[test]
    fn capped_inventory_retains_primary_documents() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("SKILL.md");
        fs::write(&path, "primary fixture").unwrap();
        fs::write(root.join("README.md"), "overview fixture").unwrap();
        for index in 0..205 {
            fs::write(root.join(format!("extra-{index:03}.md")), "extra fixture").unwrap();
        }
        let candidate = lam_session_core::SkillRead {
            name: "fixture".into(), path: path.to_string_lossy().into_owned(), workdir: None,
        };
        let detail = render("fixture", &candidate, None).unwrap();
        assert_eq!(detail["files"].as_array().unwrap().len(), 200);
        assert_eq!(detail["files_truncated"], true);
        let documents = detail["documents"].as_array().unwrap();
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0]["relative_path"], "README.md");
        assert_eq!(documents[0]["text"], "overview fixture");
        assert_eq!(documents[1]["relative_path"], "SKILL.md");
        assert_eq!(documents[1]["text"], "primary fixture");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 207);
    }

    #[test]
    fn inventory_depth_boundary_keeps_selected_documents_inside_limit() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("SKILL.md");
        fs::write(&path, "primary fixture").unwrap();
        fs::create_dir_all(root.join("a/b/c/d/e")).unwrap();
        fs::write(root.join("a/b/c/d/inside.md"), "inside fixture").unwrap();
        fs::write(root.join("a/b/c/d/e/outside.md"), "outside fixture").unwrap();
        let candidate = lam_session_core::SkillRead {
            name: "fixture".into(), path: path.to_string_lossy().into_owned(), workdir: None,
        };
        let detail = render("fixture", &candidate, Some("a/b/c/d/inside.md")).unwrap();
        assert_eq!(detail["files"].as_array().unwrap().len(), 2);
        assert_eq!(detail["files_truncated"], true);
        assert_eq!(detail["documents"].as_array().unwrap().len(), 1);
        assert_eq!(detail["documents"][0]["text"], "inside fixture");
        let excluded = render("fixture", &candidate, Some("a/b/c/d/e/outside.md")).unwrap();
        assert!(excluded["documents"].as_array().unwrap().is_empty());
        assert_eq!(fs::read_to_string(root.join("a/b/c/d/e/outside.md")).unwrap(), "outside fixture");
    }

    #[test]
    fn document_limit_preserves_raw_digest_bom_and_utf8_boundary() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("SKILL.md");
        let candidate = lam_session_core::SkillRead {
            name: "fixture".into(),
            path: path.to_string_lossy().into_owned(),
            workdir: None,
        };
        for size in [262143, 262144, 262145] {
            let mut content = vec![b'x'; size];
            content[..3].copy_from_slice(b"\xef\xbb\xbf");
            fs::write(&path, &content).unwrap();
            let detail = render("fixture", &candidate, None).unwrap();
            let documents = detail["documents"].as_array().unwrap();
            assert_eq!(documents.len(), 1);
            let document = &documents[0];
            let limit = size.min(262144);
            assert_eq!(document["bytes"], size);
            assert_eq!(document["read_bytes"], limit);
            assert_eq!(document["truncated"], size > limit);
            assert_eq!(document["text"], "x".repeat(limit - 3));
            assert_eq!(
                document["sha256"],
                format!("{:x}", Sha256::digest(&content[..limit]))
            );
            assert_eq!(fs::read(&path).unwrap(), content);
        }
        let mut content = vec![b'x'; 262143];
        content.extend_from_slice("界".as_bytes());
        fs::write(&path, &content).unwrap();
        let detail = render("fixture", &candidate, Some("SKILL.md")).unwrap();
        let document = &detail["documents"][0];
        assert_eq!(document["read_bytes"], 262144);
        assert_eq!(document["truncated"], true);
        assert_eq!(document["text"], format!("{}�", "x".repeat(262143)));
        assert_eq!(
            document["sha256"],
            format!("{:x}", Sha256::digest(&content[..262144]))
        );
        assert_eq!(fs::read(&path).unwrap(), content);
    }

    #[test]
    fn observed_unicode_skill_filename_remains_a_primary_document() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("SKILL.md");
        fs::write(&path, "fixture primary").unwrap();
        fs::write(root.join("README.md"), "fixture overview").unwrap();
        fs::write(root.join("other.md"), "fixture other").unwrap();
        fs::write(root.join("É.md"), "fixture accent").unwrap();
        fs::write(root.join("ä.md"), "fixture umlaut").unwrap();
        let arguments = json!({"cmd":format!("cat '{}'", path.display())});
        let observed = lam_session_core::skill_read_paths("exec_command", arguments.as_object());
        assert_eq!(observed.len(), 1);
        let detail = render(&observed[0].name, &observed[0], None).unwrap();
        let names = detail["documents"].as_array().unwrap().iter()
            .map(|entry| entry["relative_path"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(names, ["README.md", "SKILL.md"]);
        assert_eq!(detail["documents"][1]["text"], "fixture primary");
        let files = detail["files"].as_array().unwrap();
        assert_eq!(files[0]["relative_path"], "README.md");
        assert_eq!(files[1]["relative_path"], "SKILL.md");
        assert_eq!(files[2]["relative_path"], "other.md");
        assert_eq!(files[3]["relative_path"], "ä.md");
        assert_eq!(files[4]["relative_path"], "É.md");
    }

    #[test]
    fn skill_suffixes_follow_unicode_lowercase_for_inventory_and_documents() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().canonicalize().unwrap();
        let path = root.join("SKILL.md");
        fs::write(&path, "fixture skill").unwrap();
        let candidate = lam_session_core::SkillRead {
            name: "fixture".into(),
            path: path.to_string_lossy().into_owned(),
            workdir: None,
        };
        for (name, included, readable, format) in [
            ("secret.KEY", false, false, "key"),
            ("secret.KEY", false, false, "key"),
            ("secret.PEM", false, false, "pem"),
            ("secret.κey", true, false, "κey"),
            ("secret.İey", true, false, "i\u{307}ey"),
            ("secret.KEY.txt", true, true, "txt"),
            ("guide.MARKDOWN", true, true, "markdown"),
        ] {
            let path = root.join(name);
            fs::write(&path, "fixture content").unwrap();
            let detail = render("fixture", &candidate, Some(name)).unwrap();
            let file = detail["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["name"] == name);
            assert_eq!(file.is_some(), included, "{name}");
            if let Some(file) = file {
                assert_eq!(file["readable"], readable, "{name}");
                assert_eq!(file["format"], format, "{name}");
            }
            let documents = detail["documents"].as_array().unwrap();
            assert_eq!(documents.len(), usize::from(included && readable), "{name}");
            if included && readable {
                assert_eq!(documents[0]["text"], "fixture content");
                assert_eq!(documents[0]["relative_path"], name);
            }
            assert_eq!(fs::read_to_string(path).unwrap(), "fixture content");
        }
    }
}
