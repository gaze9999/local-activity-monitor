use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

const FRONTEND: [&str; 6] = [
    "index.html",
    "app.js",
    "style.css",
    "locales.json",
    "favicon.svg",
    "favicon.ico",
];
const WORKBENCH: [&str; 4] = [
    "workbench-ui.js",
    "workbench-ui.css",
    "workbench-ui.mjs",
    "workbench-loader.js",
];
const PIN: &str = include_str!("../../../workbench-ui.json");
const MAX_ASSET: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct Pin {
    version: u32,
    repository: String,
    revision: String,
}

#[derive(Deserialize)]
struct FrontendManifest {
    version: u32,
    inputs: String,
    workbench_revision: String,
    sha256: HashMap<String, String>,
}

#[derive(Deserialize)]
struct LibraryManifest {
    version: u32,
    repository: String,
    revision: String,
    sha256: HashMap<String, String>,
}

pub struct Assets {
    files: HashMap<&'static str, Bytes>,
    pub page_policy: String,
    pub revision: String,
}

impl Assets {
    pub fn load(root: &Path) -> io::Result<Self> {
        directory(root)?;
        let pin: Pin = serde_json::from_str(PIN).map_err(invalid)?;
        let frontend = root.join("_web");
        let library = root.join("_workbench");
        check_directory(&frontend, &FRONTEND, false)?;
        check_directory(&library, &WORKBENCH, true)?;
        let app: FrontendManifest = manifest(&frontend)?;
        let ui: LibraryManifest = manifest(&library)?;
        if pin.version != 1
            || app.version != 1
            || ui.version != 1
            || !lower_hex(&pin.revision, 40)
            || !lower_hex(&app.inputs, 64)
            || ui.repository != pin.repository
            || ui.revision != pin.revision
            || app.workbench_revision != pin.revision
        {
            return Err(invalid("Asset manifests do not match the pinned revision"));
        }
        let mut files = checked_assets(&frontend, &FRONTEND, app.sha256)?;
        files.extend(checked_assets(&library, &WORKBENCH, ui.sha256)?);
        let read_text = |name| std::str::from_utf8(&files[name]).map_err(invalid);
        let inline_text = |name| {
            read_text(name).map(|text| {
                if text.contains('\r') {
                    Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
                } else {
                    Cow::Borrowed(text)
                }
            })
        };
        let script = format!(
            "{};\n{}",
            inline_text("workbench-ui.js")?,
            inline_text("app.js")?
        )
        .replace("</", "<\\/");
        let style = format!(
            "{}\n{}",
            inline_text("workbench-ui.css")?,
            inline_text("style.css")?
        );
        let html = read_text("index.html")?
            .replace("</body>", &format!("<script>{script}</script>\n</body>"))
            .replace(
                r#"<link rel="stylesheet" href="/style.css">"#,
                &format!("<style>{style}</style>"),
            )
            .replace(r#"<link rel="stylesheet" href="/workbench-ui.css">"#, "");
        let script_hash = STANDARD.encode(Sha256::digest(script.as_bytes()));
        let style_hash = STANDARD.encode(Sha256::digest(style.as_bytes()));
        let page_policy = format!(
            "default-src 'self'; script-src 'self' 'sha256-{script_hash}'; style-src 'self' 'sha256-{style_hash}'; connect-src 'self'; img-src 'self'; frame-ancestors 'none'; base-uri 'none'"
        );
        files.insert("index.html", html.into());
        let mut digest = Sha256::new();
        for name in FRONTEND.into_iter().chain(WORKBENCH) {
            digest.update(Sha256::digest(&files[name]));
        }
        let revision = format!("{:x}", digest.finalize())[..12].to_owned();
        Ok(Self {
            files,
            page_policy,
            revision,
        })
    }

    pub fn get(&self, route: &str) -> Option<(Bytes, &'static str)> {
        let (name, mime) = match route {
            "/" => ("index.html", "text/html; charset=utf-8"),
            "/app.js" => ("app.js", "text/javascript; charset=utf-8"),
            "/style.css" => ("style.css", "text/css; charset=utf-8"),
            "/locales.json" => ("locales.json", "application/json; charset=utf-8"),
            "/favicon.svg" => ("favicon.svg", "image/svg+xml"),
            "/favicon.ico" => ("favicon.ico", "image/vnd.microsoft.icon"),
            "/workbench-ui.js" => ("workbench-ui.js", "text/javascript; charset=utf-8"),
            "/workbench-ui.css" => ("workbench-ui.css", "text/css; charset=utf-8"),
            _ => return None,
        };
        Some((self.files.get(name)?.clone(), mime))
    }
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.is_symlink()
}

fn directory(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if redirected(&meta) || !meta.is_dir() {
        return Err(invalid("Unsafe asset directory"));
    }
    Ok(())
}

fn check_directory(root: &Path, files: &[&str], python_bundle: bool) -> io::Result<()> {
    directory(root)?;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| invalid("Invalid asset name"))?;
        if python_bundle && name == "__pycache__" {
            directory(&entry.path())?;
            continue;
        }
        if name != "manifest.json"
            && !files.contains(&name)
            && !(python_bundle && name == "__init__.py")
        {
            return Err(invalid("Unknown generated asset"));
        }
        let meta = fs::symlink_metadata(entry.path())?;
        if redirected(&meta) || !meta.is_file() {
            return Err(invalid("Unsafe generated asset"));
        }
    }
    Ok(())
}

fn read_file(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if redirected(&meta) || !meta.is_file() || meta.len() > limit {
        return Err(invalid("Unsafe or oversized generated asset"));
    }
    let mut data = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err(invalid("Oversized generated asset"));
    }
    Ok(data)
}

fn manifest<T: serde::de::DeserializeOwned>(root: &Path) -> io::Result<T> {
    serde_json::from_slice(&read_file(&root.join("manifest.json"), 65_536)?).map_err(invalid)
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn checked_assets(
    root: &Path,
    files: &[&'static str],
    checksums: HashMap<String, String>,
) -> io::Result<HashMap<&'static str, Bytes>> {
    let expected = files.iter().copied().collect::<HashSet<_>>();
    if checksums.keys().map(String::as_str).collect::<HashSet<_>>() != expected {
        return Err(invalid("Invalid asset checksum list"));
    }
    files
        .iter()
        .map(|&name| {
            let data = read_file(&root.join(name), MAX_ASSET)?;
            if format!("{:x}", Sha256::digest(&data)) != checksums[name] {
                return Err(invalid(format!("Asset checksum mismatch: {name}")));
            }
            Ok((name, data.into()))
        })
        .collect()
}
