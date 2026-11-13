//! Read metadata only for an event selected by the observed-file index.
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub(crate) fn observed(event: &Value) -> Value {
    let Some(name) = event.get("path").and_then(Value::as_str) else {
        return json!({"modified_at":null,"bytes":null,"health":"rejected"});
    };
    let path = Path::new(name);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(workdir) = event.get("workdir").and_then(Value::as_str) {
        Path::new(workdir).join(path)
    } else {
        path.to_path_buf()
    };
    let name = joined
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let extension = joined
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let rendered = joined.to_string_lossy();
    if !joined.is_absolute()
        || rendered.starts_with("\\\\")
        || rendered.starts_with("//")
        || ["auth.json", "credentials.json", ".env"].contains(&name.as_str())
        || ["pem", "key"].contains(&extension.as_str())
    {
        return json!({"modified_at":null,"bytes":null,"health":"rejected"});
    }
    for part in joined.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(meta) if super::redirected(&meta) => {
                return json!({"modified_at":null,"bytes":null,"health":"rejected"})
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return json!({"modified_at":null,"bytes":null,"health":"unavailable"}),
        }
    }
    match fs::metadata(&joined) {
        Ok(meta) if meta.is_file() => {
            let modified = meta
                .modified()
                .ok()
                .map(|time| super::project::stamp(time.into()));
            json!({"modified_at":modified,"bytes":meta.len(),"health":"ok"})
        }
        Ok(_) => json!({"modified_at":null,"bytes":null,"health":"missing"}),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            json!({"modified_at":null,"bytes":null,"health":"missing"})
        }
        Err(_) => json!({"modified_at":null,"bytes":null,"health":"unavailable"}),
    }
}
