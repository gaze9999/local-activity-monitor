//! Bounded local GPU metadata projection.
use regex::Regex;
use serde_json::{json, Value};
use std::sync::LazyLock;
static VRAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+(?:\.\d+)?)\s*(MB|GB|MiB|GiB)$").unwrap());
static APPLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Apple M\d").unwrap());
fn mac_projection(value: &Value) -> Vec<Value> {
    let Some(items) = value.get("SPDisplaysDataType").and_then(Value::as_array) else {
        return Vec::new();
    };
    items.iter().take(16).filter_map(|item|{
        let name=item.get("sppci_model").and_then(Value::as_str).filter(|name|!name.is_empty()).or_else(||item.get("_name").and_then(Value::as_str))?;
        let capacity=item.get("spdisplays_vram").and_then(Value::as_str).and_then(|memory|VRAM.captures(memory)).and_then(|parts|{
            let number=parts[1].parse::<f64>().ok()?;
            let multiplier=if matches!(&parts[2],"MB"|"MiB"){1024f64.powi(2)}else{1024f64.powi(3)};
            let bytes=number*multiplier;(bytes.is_finite()&&bytes>=0.0&&bytes<(1u64<<63) as f64).then_some(bytes as u64)
        });
        let unified=APPLE.is_match(name);
        Some(json!({"name":name.chars().take(160).collect::<String>(),"dedicated_memory_bytes":if unified {None}else{capacity},"shared_memory_bytes":null,"driver_version":null,"memory_type":if unified {Some("unified")}else{None}}))
    }).collect()
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn command(program: &str, args: &[&str], duration: std::time::Duration) -> Option<String> {
    use std::process::Stdio;
    use tokio::{io::AsyncReadExt, process::Command, time::timeout};
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let result = timeout(duration, async {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = stdout.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            if bytes.len().checked_add(n)? > 524292 {
                return None;
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        if !child.wait().await.ok()?.success() {
            return None;
        }
        let text = String::from_utf8(bytes).ok()?;
        if text.chars().count() > 128 * 1024 {
            return None;
        }
        Some(text)
    })
    .await
    .ok()
    .flatten();
    if result.is_none() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}
#[cfg(target_os = "macos")]
async fn native() -> Vec<Value> {
    let Some(output) = command(
        "/usr/sbin/system_profiler",
        &["-json", "-detailLevel", "mini", "SPDisplaysDataType"],
        std::time::Duration::from_secs(2),
    )
    .await
    else {
        return Vec::new();
    };
    serde_json::from_str::<Value>(&output)
        .ok()
        .map(|value| mac_projection(&value))
        .unwrap_or_default()
}
#[cfg(any(target_os = "linux", test))]
fn nvidia_projection(output: &str) -> (Vec<Value>, std::collections::BTreeSet<String>) {
    let mut result = Vec::new();
    let mut slots = std::collections::BTreeSet::new();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(output.as_bytes());
    for row in reader.records().take(16).flatten() {
        if row.len() != 4 {
            continue;
        }
        let name = row[0].trim();
        let capacity = row[1]
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|value| *value < i64::MAX as u64 + 1)
            .map(|value| u128::from(value) * 1024 * 1024);
        let driver = row[2].trim();
        let slot = row[3].trim().to_lowercase();
        slots.insert(slot.trim_start_matches('0').to_owned());
        result.push(json!({"name":if name.is_empty(){None}else{Some(name.chars().take(160).collect::<String>())},"dedicated_memory_bytes":capacity,"shared_memory_bytes":null,"driver_version":if ["","N/A","[N/A]"].contains(&driver){None}else{Some(driver)}}));
    }
    (result, slots)
}
#[cfg(target_os = "linux")]
fn read_small(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    Some(text.chars().take(256).collect::<String>().trim().to_owned())
}
#[cfg(target_os = "linux")]
async fn native() -> Vec<Value> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(3);
    let output = command(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total,driver_version,pci.bus_id",
            "--format=csv,noheader,nounits",
        ],
        Duration::from_secs(2),
    )
    .await
    .unwrap_or_default();
    let (mut gpus, slots) = nvidia_projection(&output);
    let Ok(cards) = std::fs::read_dir("/sys/class/drm") else {
        return gpus;
    };
    let slot_pattern =
        Regex::new(r"^[0-9a-fA-F]{4}:[0-9a-fA-F]{2}:[0-9a-fA-F]{2}\.[0-7]$").unwrap();
    let card_pattern = Regex::new(r"^card[0-9]+$").unwrap();
    let driver_pattern = Regex::new(r"^[\w-]{1,80}$").unwrap();
    for card in cards
        .take(64)
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| card_pattern.is_match(name))
        })
        .take(16)
    {
        let Ok(device) = card.path().join("device").canonicalize() else {
            continue;
        };
        let Some(slot) = device.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if slots.contains(slot.to_lowercase().trim_start_matches('0')) {
            continue;
        }
        let mut name = read_small(&device.join("product_name")).filter(|name| !name.is_empty());
        if name.is_none() && slot_pattern.is_match(slot) && Instant::now() < deadline {
            let timeout = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(2));
            if let Some(output) = command("lspci", &["-mm", "-s", slot], timeout).await {
                if let Some(fields) = shlex::split(&output) {
                    if fields.len() >= 4 {
                        name = Some(
                            format!("{} {}", fields[2], fields[3])
                                .chars()
                                .take(160)
                                .collect(),
                        );
                    }
                }
            }
        }
        let driver = device
            .join("driver")
            .canonicalize()
            .ok()
            .and_then(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned)
            })
            .filter(|value| driver_pattern.is_match(value));
        let version = driver
            .and_then(|driver| {
                read_small(
                    &std::path::Path::new("/sys/module")
                        .join(driver)
                        .join("version"),
                )
            })
            .filter(|value| !value.is_empty());
        let capacity = read_small(&device.join("mem_info_vram_total"))
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value < i64::MAX as u64 + 1);
        gpus.push(json!({"name":name,"dedicated_memory_bytes":capacity,"shared_memory_bytes":null,"driver_version":version}));
        if gpus.len() >= 16 {
            break;
        }
    }
    gpus.truncate(16);
    gpus
}
#[cfg(windows)]
async fn native() -> Vec<Value> {
    tokio::task::spawn_blocking(crate::gpu_windows::collect)
        .await
        .unwrap_or_default()
}
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
async fn native() -> Vec<Value> {
    Vec::new()
}
#[cfg(any(windows, test))]
#[derive(Default)]
pub(crate) struct AdapterFilter {
    seen: std::collections::BTreeSet<(u32, u32)>,
    physical: std::collections::BTreeMap<(u32, u32, u32), usize>,
}
#[cfg(any(windows, test))]
impl AdapterFilter {
    pub(crate) fn accept(
        &mut self,
        flags: u32,
        luid: (u32, u32),
        device: (u32, u32, u32),
        counts: Option<&std::collections::BTreeMap<(u32, u32, u32), usize>>,
    ) -> bool {
        if flags & 3 != 0 {
            return false;
        }
        if luid != (0, 0) && !self.seen.insert(luid) {
            return false;
        }
        if let Some(limit) = counts
            .and_then(|counts| counts.get(&device))
            .filter(|limit| **limit > 0)
        {
            let used = self.physical.entry(device).or_default();
            if *used >= *limit {
                return false;
            }
            *used += 1;
        }
        true
    }
}
#[cfg(any(windows, test))]
pub(crate) fn pci_identity(value: &str) -> Option<(u32, u32, u32)> {
    static PCI: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^PCI\\VEN_([0-9a-f]{4})&DEV_([0-9a-f]{4})&SUBSYS_([0-9a-f]{8})").unwrap()
    });
    let capture = PCI.captures(value)?;
    Some((
        u32::from_str_radix(&capture[1], 16).ok()?,
        u32::from_str_radix(&capture[2], 16).ok()?,
        u32::from_str_radix(&capture[3], 16).ok()?,
    ))
}
pub async fn collect() -> Vec<Value> {
    native().await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mac_projection_bounds_fields_and_handles_unified_memory() {
        let rows = json!({"SPDisplaysDataType":[{"sppci_model":"Apple M4","spdisplays_vram":"16 GB","path":"PRIVATE"},{"_name":"Other GPU","spdisplays_vram":"1.5 GiB"},{"_name":4}]});
        let result = mac_projection(&rows);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["memory_type"], "unified");
        assert!(result[0]["dedicated_memory_bytes"].is_null());
        assert_eq!(result[1]["dedicated_memory_bytes"], 1610612736u64);
        assert!(!json!(result).to_string().contains("PRIVATE"));
    }
    #[test]
    fn linux_nvidia_projection_handles_quoted_names_and_slot_deduplication() {
        let (items, slots) = nvidia_projection(
            "\"GPU, Example\", 1024, 550.1, 0000:01:00.0\nOther, bad, N/A, 0000:02:00.0\ninvalid\n",
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "GPU, Example");
        assert_eq!(items[0]["dedicated_memory_bytes"], 1073741824u64);
        assert!(items[1]["dedicated_memory_bytes"].is_null());
        assert!(items[1]["driver_version"].is_null());
        assert!(slots.contains(":01:00.0"));
    }
    #[test]
    fn windows_adapter_filter_preserves_physical_counts_and_fallbacks() {
        let id = (0x10de, 0x2684, 0x12345678);
        assert_eq!(
            pci_identity(r"PCI\VEN_10DE&DEV_2684&SUBSYS_12345678&REV_A1"),
            Some(id)
        );
        assert_eq!(pci_identity(r"USB\VEN_10DE&DEV_2684&SUBSYS_12345678"), None);
        let counts = std::collections::BTreeMap::from([(id, 2)]);
        let mut filter = AdapterFilter::default();
        assert!(!filter.accept(2, (1, 1), id, Some(&counts)));
        assert!(filter.accept(0, (1, 1), id, Some(&counts)));
        assert!(!filter.accept(0, (1, 1), id, Some(&counts)));
        assert!(filter.accept(0, (2, 2), id, Some(&counts)));
        assert!(!filter.accept(0, (3, 3), id, Some(&counts)));
        assert!(filter.accept(0, (4, 4), (0x8086, 1, 2), Some(&counts)));
        let mut fallback = AdapterFilter::default();
        assert!(fallback.accept(0, (0, 0), id, None));
        assert!(fallback.accept(0, (0, 0), id, None));
    }
}
