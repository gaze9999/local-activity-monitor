//! Bounded font-family enumeration, excluding font paths.
use std::collections::BTreeSet;

fn clean(families: impl IntoIterator<Item = String>) -> Vec<String> {
    families
        .into_iter()
        .filter(|value| {
            !value.is_empty()
                && value.chars().count() <= 160
                && value.chars().all(|ch| {
                    ch.is_alphanumeric() || ch == '_' || ch.is_whitespace() || "' .-".contains(ch)
                })
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(500)
        .collect()
}

#[cfg(target_os = "macos")]
fn native() -> Vec<String> {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "CoreText", kind = "framework")]
    extern "C" {
        fn CTFontManagerCopyAvailableFontFamilyNames() -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFStringGetCString(
            string: *const c_void,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(value: *const c_void);
    }
    let mut result = Vec::new();
    // The copied array owns the strings until CFRelease and all output buffers are bounded.
    unsafe {
        let array = CTFontManagerCopyAvailableFontFamilyNames();
        if array.is_null() {
            return result;
        }
        let count = CFArrayGetCount(array).clamp(0, 2000);
        for index in 0..count {
            let item = CFArrayGetValueAtIndex(array, index);
            if item.is_null() {
                continue;
            }
            let mut buffer = [0 as c_char; 1024];
            if CFStringGetCString(item, buffer.as_mut_ptr(), buffer.len() as isize, 0x08000100) != 0
            {
                if let Ok(value) = CStr::from_ptr(buffer.as_ptr()).to_str() {
                    result.push(value.to_owned());
                }
            }
        }
        CFRelease(array);
    }
    result
}

#[cfg(windows)]
fn native() -> Vec<String> {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, EnumFontFamiliesExW, DEFAULT_CHARSET, LOGFONTW, TEXTMETRICW,
    };
    unsafe extern "system" fn collect(
        font: *const LOGFONTW,
        _: *const TEXTMETRICW,
        _: u32,
        context: isize,
    ) -> i32 {
        if font.is_null() || context == 0 {
            return 0;
        }
        let values = &mut *(context as *mut BTreeSet<String>);
        let face = &(*font).lfFaceName;
        let end = face
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(face.len());
        if let Ok(name) = String::from_utf16(&face[..end]) {
            values.insert(name.trim_start_matches('@').to_owned());
        }
        i32::from(values.len() < 2000)
    }
    let mut result = BTreeSet::new();
    // The callback only accesses the live set and device context is released after enumeration.
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if !dc.is_null() {
            let mut font = LOGFONTW::default();
            font.lfCharSet = DEFAULT_CHARSET;
            EnumFontFamiliesExW(
                dc,
                &font,
                Some(collect),
                (&mut result as *mut BTreeSet<String>) as isize,
                0,
            );
            DeleteDC(dc);
        }
    }
    if result.is_empty() {
        result.extend(registry_fonts());
    }
    result.into_iter().collect()
}
#[cfg(windows)]
fn registry_fonts() -> Vec<String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ,
    };
    let path: Vec<u16> = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts\0"
        .encode_utf16()
        .collect();
    let mut result = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let mut key: HKEY = std::ptr::null_mut();
        // Fixed read-only registry location, no values or file paths are read.
        if unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ, &mut key) } != 0 {
            continue;
        }
        for index in 0..2000 {
            let mut name = [0u16; 1024];
            let mut length = name.len() as u32;
            let status = unsafe {
                RegEnumValueW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != 0 {
                break;
            }
            if let Ok(label) = String::from_utf16(&name[..length as usize]) {
                let label = label.trim_end();
                let label = if label.ends_with(')') {
                    label
                        .rsplit_once('(')
                        .map(|(name, _)| name.trim_end())
                        .unwrap_or(label)
                } else {
                    label
                };
                result.extend(label.split(" & ").map(|name| name.trim().to_owned()));
            }
        }
        unsafe { RegCloseKey(key) };
    }
    result
}

#[cfg(any(target_os = "linux", test))]
fn fontconfig_output(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .chars()
        .take(262144)
        .collect::<String>()
        .lines()
        .flat_map(|line| {
            line.split(',')
                .map(|name| name.trim().to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(target_os = "linux")]
async fn read_linux() -> Vec<String> {
    use std::process::Stdio;
    use tokio::{
        io::AsyncReadExt,
        process::Command,
        time::{timeout, Duration},
    };
    let Ok(mut child) = Command::new("fc-list")
        .args(["--format", "%{family}\n"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return Vec::new();
    };
    let Some(mut stdout) = child.stdout.take() else {
        return Vec::new();
    };
    let result = timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = stdout.read(&mut chunk).await.ok()?;
            if read == 0 {
                break;
            }
            let remaining = (262144usize * 4 + 4).saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..read.min(remaining)]);
        }
        if !child.wait().await.ok()?.success() {
            return None;
        }
        Some(bytes)
    })
    .await
    .ok()
    .flatten();
    if result.is_none() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
        .map(|bytes| fontconfig_output(&bytes))
        .unwrap_or_default()
}
pub async fn collect() -> Vec<String> {
    #[cfg(any(target_os = "macos", windows))]
    let values = tokio::task::spawn_blocking(native)
        .await
        .unwrap_or_default();
    #[cfg(target_os = "linux")]
    let values = read_linux().await;
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let values = Vec::new();
    clean(values)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitizes_deduplicates_and_bounds_names() {
        let mut input = vec![
            "Noto Sans".into(),
            "Noto Sans".into(),
            "/private/file".into(),
            "字型一".into(),
            "bad:name".into(),
            "x".repeat(161),
        ];
        input.extend((0..600).map(|i| format!("Font {i:03}")));
        let values = clean(input);
        assert_eq!(values.len(), 500);
        assert!(!values.iter().any(|v| v.contains('/') || v.contains(':')));
        assert!(values.windows(2).all(|pair| pair[0] < pair[1]));
    }
    #[test]
    fn fontconfig_splits_family_aliases_and_bounds_characters() {
        assert_eq!(
            fontconfig_output("字型, Font Two\nOther\n".as_bytes()),
            ["字型", "Font Two", "Other"]
        );
        let output = format!("{}Tail", "A".repeat(262144));
        assert_eq!(fontconfig_output(output.as_bytes())[0].len(), 262144);
    }
    #[tokio::test]
    async fn native_result_obeys_bounds_and_contains_no_paths() {
        let values = collect().await;
        assert!(values.len() <= 500);
        assert!(values
            .iter()
            .all(|value| !value.contains('/') && value.chars().count() <= 160));
    }
}
