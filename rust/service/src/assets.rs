use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static WORKBENCH_VERSION: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"global\.WorkbenchUI\s*=\s*Object\.freeze\(\{\s*version:\s*"((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))""#).expect("Workbench UI version pattern")
});

fn compatible_workbench(source: &str) -> bool {
    let Some(version) = WORKBENCH_VERSION
        .captures(source)
        .and_then(|capture| capture.get(1))
    else {
        return false;
    };
    let mut parts = version.as_str().split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    major != "0" || minor.len() > 2 || minor.len() == 2 && minor >= "13"
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn development_path_expands_named_user_from_system_account() {
        let mut buffer = vec![0u8; 65_536];
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        assert_eq!(status, 0);
        assert!(!result.is_null());
        let name = unsafe { std::ffi::CStr::from_ptr((*result).pw_name) }
            .to_str()
            .unwrap();
        let home = unsafe { std::ffi::CStr::from_ptr((*result).pw_dir) }
            .to_bytes()
            .to_vec();
        let expected = PathBuf::from(std::ffi::OsString::from_vec(home)).join("fixture");
        assert_eq!(
            expand_development_path(PathBuf::from(format!("~{name}/fixture"))).unwrap(),
            expected
        );
        assert_eq!(
            expand_development_path(PathBuf::from("relative/fixture")).unwrap(),
            PathBuf::from("relative/fixture")
        );
    }
}

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DevelopmentSelection {
    version: u32,
    source: PathBuf,
    revision: String,
}

#[cfg(unix)]
fn named_home(user: &std::ffi::OsStr) -> io::Result<PathBuf> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let user = std::ffi::CString::new(user.as_bytes()).map_err(invalid)?;
    let mut size = 16_384usize;
    loop {
        let mut buffer = vec![0u8; size];
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwnam_r(
                user.as_ptr(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE && size < 1_048_576 {
            size *= 2;
            continue;
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status));
        }
        if result.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Unknown home directory",
            ));
        }
        let home = unsafe { (*result).pw_dir };
        if home.is_null() {
            return Err(invalid("Missing home directory"));
        }
        let bytes = unsafe { std::ffi::CStr::from_ptr(home) }
            .to_bytes()
            .to_vec();
        return Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)));
    }
}

pub(crate) fn expand_development_path(path: PathBuf) -> io::Result<PathBuf> {
    let Some(std::path::Component::Normal(first)) = path.components().next() else {
        return Ok(path);
    };
    let Some(first_text) = first.to_str() else {
        return Ok(path);
    };
    let Some(user) = first_text.strip_prefix('~') else {
        return Ok(path);
    };
    #[cfg(unix)]
    let home = if user.is_empty() {
        crate::platform_home().ok_or_else(|| invalid("Missing home directory"))?
    } else {
        named_home(std::ffi::OsStr::new(user))?
    };
    #[cfg(windows)]
    let home = {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOMEPATH").map(|path| {
                    PathBuf::from(std::env::var_os("HOMEDRIVE").unwrap_or_default()).join(path)
                })
            })
            .ok_or_else(|| invalid("Missing home directory"))?;
        let current = std::env::var_os("USERNAME");
        if !user.is_empty() && current.as_deref() != Some(std::ffi::OsStr::new(user)) {
            if home.file_name() != current.as_deref() {
                return Err(invalid("Unknown home directory"));
            }
            home.parent()
                .ok_or_else(|| invalid("Unknown home directory"))?
                .join(user)
        } else {
            home
        }
    };
    #[cfg(not(any(unix, windows)))]
    let home = if user.is_empty() {
        crate::platform_home().ok_or_else(|| invalid("Missing home directory"))?
    } else {
        return Err(invalid("Unknown home directory"));
    };
    Ok(home.join(path.strip_prefix(first).map_err(invalid)?))
}

fn development_source(root: &Path, pin: &Pin) -> io::Result<Option<PathBuf>> {
    let Some(project) = root.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    if !project.join("workbench-ui.json").is_file() {
        return Ok(None);
    }
    let config_dir = project.join(".local");
    let config = config_dir.join("workbench-ui-development.json");
    let selected = match fs::symlink_metadata(&config) {
        Ok(_) => {
            directory(&config_dir)?;
            if config_dir.canonicalize()? != config_dir {
                return Err(invalid("Unsafe development selection directory"));
            }
            let value: DevelopmentSelection =
                serde_json::from_slice(&read_file(&config, 4096)?).map_err(invalid)?;
            if value.version != 1
                || value.revision != pin.revision
                || !value.source.is_absolute()
                || fs::symlink_metadata(&value.source)
                    .map_or(true, |metadata| redirected(&metadata))
                || value.source.canonicalize()? != value.source
            {
                return Err(invalid("Invalid Workbench UI development selection"));
            }
            Some(value.source)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let configured = std::env::var_os("WORKBENCH_UI_PATH").map(PathBuf::from);
    let fallback = project
        .parent()
        .map(|parent| parent.join("workbench-ui"))
        .filter(|source| {
            !root.join("_workbench").exists()
                && source
                    .join("integrations/python/workbench_assets.py")
                    .is_file()
        });
    let Some(path) = configured.or(selected).or(fallback) else {
        return Ok(None);
    };
    let path = expand_development_path(path)?;
    let source = path.canonicalize()?;
    let loader = source.join("integrations/python/workbench_assets.py");
    let loader_metadata = fs::symlink_metadata(&loader)?;
    if redirected(&loader_metadata)
        || !loader_metadata.is_file()
        || loader.canonicalize()?.parent() != Some(source.join("integrations/python").as_path())
    {
        return Err(invalid("Unsafe Workbench UI loader"));
    }
    Ok(Some(source.join("src")))
}

pub struct Assets {
    files: HashMap<&'static str, Bytes>,
    pub page_policy: String,
    pub revision: String,
}

impl Assets {
    pub fn load(root: &Path) -> io::Result<Self> {
        directory(root)?;
        let resolved_root = root.canonicalize()?;
        let root = resolved_root.as_path();
        let pin: Pin = serde_json::from_str(PIN).map_err(invalid)?;
        let frontend = root.join("_web");
        let library = root.join("_workbench");
        check_directory(&frontend, &FRONTEND, false)?;
        let app: FrontendManifest = manifest(&frontend)?;
        if pin.version != 1
            || app.version != 1
            || !lower_hex(&pin.revision, 40)
            || !lower_hex(&app.inputs, 64)
            || app.workbench_revision != pin.revision
        {
            return Err(invalid("Asset manifests do not match the pinned revision"));
        }
        let mut files = checked_assets(&frontend, &FRONTEND, app.sha256)?;
        if let Some(source) = development_source(root, &pin)? {
            directory(&source)?;
            for name in WORKBENCH {
                files.insert(name, read_file(&source.join(name), MAX_ASSET)?.into());
            }
        } else {
            check_directory(&library, &WORKBENCH, true)?;
            let ui: LibraryManifest = manifest(&library)?;
            if ui.version != 1 || ui.repository != pin.repository || ui.revision != pin.revision {
                return Err(invalid("Asset manifests do not match the pinned revision"));
            }
            files.extend(checked_assets(&library, &WORKBENCH, ui.sha256)?);
        }
        let read_text = |name| std::str::from_utf8(&files[name]).map_err(invalid);
        if !compatible_workbench(read_text("workbench-ui.js")?) {
            return Err(invalid("Workbench UI 0.13.0 or later is required"));
        }
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
