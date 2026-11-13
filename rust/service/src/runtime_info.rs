//! Read process and operating-system identity without launching external commands.
use serde_json::{json, Value};

pub fn snapshot() -> Value {
    let mut value = json!({
        "platform": match std::env::consts::OS { "macos"=>"Darwin", "windows"=>"Windows", "linux"=>"Linux", other=>other },
        "architecture":std::env::consts::ARCH,
        "process_bits":std::mem::size_of::<usize>() * 8,
        "logical_cpus":std::thread::available_parallelism().ok().map(|count|count.get()),
        "system_release":null,"system_version":null,"processor":processor_name(),"gpus":[],
    });
    #[cfg(unix)]
    {
        let mut output = std::mem::MaybeUninit::<libc::utsname>::uninit();
        // uname writes an initialized utsname when it returns zero.
        if unsafe { libc::uname(output.as_mut_ptr()) } == 0 {
            let output = unsafe { output.assume_init() };
            let field = |raw: &[libc::c_char]| {
                let bytes: Vec<u8> = raw
                    .iter()
                    .take_while(|&&byte| byte != 0)
                    .map(|&byte| byte as u8)
                    .collect();
                String::from_utf8(bytes)
                    .ok()
                    .filter(|text| !text.is_empty())
            };
            if let Some(machine) = field(&output.machine) {
                value["architecture"] = json!(machine);
            }
            value["system_release"] = json!(field(&output.release));
            value["system_version"] = json!(field(&output.version));
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GetNativeSystemInfo, SYSTEM_INFO};
        let mut system = SYSTEM_INFO::default();
        // GetNativeSystemInfo initializes SYSTEM_INFO with the machine architecture.
        unsafe {
            GetNativeSystemInfo(&mut system);
        }
        let architecture = unsafe { system.Anonymous.Anonymous.wProcessorArchitecture };
        if let Some(name) = windows_architecture(architecture) {
            value["architecture"] = json!(name);
        }
        if let Some((release, version)) = windows_version() {
            value["system_release"] = json!(release);
            value["system_version"] = json!(version);
        }
    }
    value
}

#[cfg(any(windows, test))]
fn windows_architecture(code: u16) -> Option<&'static str> {
    match code {
        0 => Some("x86"),
        5 => Some("ARM"),
        6 => Some("ia64"),
        9 => Some("AMD64"),
        12 => Some("ARM64"),
        _ => None,
    }
}

#[cfg(any(windows, test))]
fn windows_release(major: u32, minor: u32, build: u32, client: bool) -> Option<&'static str> {
    const CLIENT: &[((u32, u32, u32), &str)] = &[
        ((10, 1, 0), "post11"),
        ((10, 0, 22000), "11"),
        ((6, 4, 0), "10"),
        ((6, 3, 0), "8.1"),
        ((6, 2, 0), "8"),
        ((6, 1, 0), "7"),
        ((6, 0, 0), "Vista"),
        ((5, 2, 3790), "XP64"),
        ((5, 2, 0), "XPMedia"),
        ((5, 1, 0), "XP"),
        ((5, 0, 0), "2000"),
    ];
    const SERVER: &[((u32, u32, u32), &str)] = &[
        ((10, 1, 0), "post2025Server"),
        ((10, 0, 26100), "2025Server"),
        ((10, 0, 20348), "2022Server"),
        ((10, 0, 17763), "2019Server"),
        ((6, 4, 0), "2016Server"),
        ((6, 3, 0), "2012ServerR2"),
        ((6, 2, 0), "2012Server"),
        ((6, 1, 0), "2008ServerR2"),
        ((6, 0, 0), "2008Server"),
        ((5, 2, 0), "2003Server"),
        ((5, 0, 0), "2000Server"),
    ];
    (if client { CLIENT } else { SERVER })
        .iter()
        .find(|(version, _)| *version <= (major, minor, build))
        .map(|(_, name)| *name)
}

#[cfg(windows)]
fn windows_version() -> Option<(Option<&'static str>, String)> {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOEXW;
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(version: *mut OSVERSIONINFOEXW) -> i32;
    }
    let mut info = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    // RtlGetVersion initializes the supplied structure and is not affected by application manifests.
    if unsafe { RtlGetVersion(&mut info) } != 0 {
        return None;
    }
    Some((
        windows_release(
            info.dwMajorVersion,
            info.dwMinorVersion,
            info.dwBuildNumber,
            info.wProductType == 1,
        ),
        format!(
            "{}.{}.{}",
            info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_actual_process_runtime_without_external_commands() {
        let value = snapshot();
        assert!(value["platform"]
            .as_str()
            .is_some_and(|text| !text.is_empty()));
        assert!(value["architecture"]
            .as_str()
            .is_some_and(|text| !text.is_empty()));
        assert_eq!(value["process_bits"], std::mem::size_of::<usize>() * 8);
        assert!(value["logical_cpus"].as_u64().is_none_or(|count| count > 0));
        #[cfg(unix)]
        assert!(value["system_release"]
            .as_str()
            .is_some_and(|text| !text.is_empty()));
    }

    #[test]
    fn windows_release_and_architecture_match_selected_platform_mapping() {
        assert_eq!(windows_release(10, 0, 22000, true), Some("11"));
        assert_eq!(windows_release(10, 0, 19045, true), Some("10"));
        assert_eq!(windows_release(10, 0, 20348, false), Some("2022Server"));
        assert_eq!(windows_release(10, 0, 26100, false), Some("2025Server"));
        assert_eq!(windows_release(4, 0, 0, true), None);
        assert_eq!(windows_architecture(9), Some("AMD64"));
        assert_eq!(windows_architecture(12), Some("ARM64"));
        assert_eq!(windows_architecture(65535), None);
    }

    #[test]
    fn memory_parser_checks_fields_and_overflow() {
        assert_eq!(
            parse_meminfo("MemTotal: 2048 kB\nMemAvailable: 512 kB\n"),
            (Some(2_097_152), Some(524_288))
        );
        assert_eq!(
            parse_meminfo("MemTotal: invalid kB\nMemAvailable: 18446744073709551615 kB\n"),
            (None, None)
        );
    }

    #[test]
    fn cpu_sampling_rejects_invalid_deltas_and_parses_bounded_counts() {
        assert_eq!(parse_cpu_times("cpu 10 2 8 50 5 3 2 1 99"), Some((55, 81)));
        assert_eq!(parse_cpu_times("cpu 10 2 bad 50"), None);
        assert_eq!(parse_cpu_times("cpu0 10 2 8 50"), None);
        assert_eq!(cpu_percent(Some((50, 100)), Some((60, 150))), Some(80.0));
        assert_eq!(cpu_percent(Some((50, 100)), Some((40, 150))), None);
        assert_eq!(cpu_percent(Some((50, 100)), Some((60, 100))), None);
        assert_eq!(cpu_percent(Some((50, 100)), Some((90, 110))), None);
        assert_eq!(cpu_percent(None, Some((90, 110))), None);
    }

    #[test]
    fn cpu_sample_is_bounded_and_reports_scope() {
        let mut memory = Memory::new();
        memory.checked = std::time::Instant::now() - std::time::Duration::from_secs(6);
        let value = memory.snapshot();
        assert!(value["cpu_usage_percent"]
            .as_f64()
            .is_none_or(|v| (0.0..=100.0).contains(&v)));
        assert!(["system", "processor_group"].contains(&value["cpu_scope"].as_str().unwrap()));
    }

    #[test]
    fn device_completion_records_only_the_first_complete_state() {
        let mut status = DeviceCompletion::default();
        assert!(status.ready_at().is_none());
        status.gpus_finished();
        assert!(status.ready_at().is_none());
        status.fonts_finished();
        let ready = status.ready_at().unwrap();
        status.fonts_finished();
        status.gpus_finished();
        assert_eq!(status.ready_at(), Some(ready));
        let mut reverse = DeviceCompletion::default();
        reverse.fonts_finished();
        assert!(reverse.ready_at().is_none());
        reverse.gpus_finished();
        assert!(reverse.ready_at().is_some());
    }
}

#[cfg(target_os = "macos")]
fn sysctl_value(name: &[u8], output: &mut [u8]) -> Option<usize> {
    let mut length = output.len();
    // The key is a NUL-terminated constant and the buffer has the declared capacity.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr().cast(),
            output.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && length <= output.len()).then_some(length)
}

fn limited_name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.chars().take(160).collect())
}

#[cfg(target_os = "macos")]
fn processor_name() -> Option<String> {
    let mut value = [0u8; 1024];
    let length = sysctl_value(b"machdep.cpu.brand_string\0", &mut value)?;
    let end = value[..length]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(length);
    limited_name(std::str::from_utf8(&value[..end]).ok()?)
}
#[cfg(target_os = "linux")]
fn processor_name() -> Option<String> {
    use std::io::Read;
    let mut data = String::new();
    std::fs::File::open("/proc/cpuinfo")
        .ok()?
        .take(16384)
        .read_to_string(&mut data)
        .ok()?;
    data.lines()
        .find(|line| line.starts_with("model name"))
        .and_then(|line| line.split_once(':'))
        .and_then(|(_, value)| limited_name(value))
}
#[cfg(windows)]
fn processor_name() -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let path: Vec<u16> = "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0\0"
        .encode_utf16()
        .collect();
    let key: Vec<u16> = "ProcessorNameString\0".encode_utf16().collect();
    let mut data = [0u16; 512];
    let mut length = std::mem::size_of_val(&data) as u32;
    // Read the fixed registry value into a bounded UTF-16 buffer.
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            key.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut length,
        )
    };
    if result != 0 {
        return None;
    }
    let end = data
        .iter()
        .position(|&character| character == 0)
        .unwrap_or(data.len());
    limited_name(&String::from_utf16(&data[..end]).ok()?)
}
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn processor_name() -> Option<String> {
    None
}

#[cfg(any(target_os = "linux", test))]
fn parse_meminfo(data: &str) -> (Option<u64>, Option<u64>) {
    let field = |key: &str| {
        data.lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| *name == key)
            .and_then(|(_, value)| value.split_ascii_whitespace().next())?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024)
    };
    (field("MemTotal"), field("MemAvailable"))
}
fn memory_info() -> (Option<u64>, Option<u64>) {
    #[cfg(target_os = "macos")]
    {
        let mut data = [0u8; 8];
        let total = sysctl_value(b"hw.memsize\0", &mut data)
            .filter(|length| *length == 8)
            .map(|_| u64::from_ne_bytes(data));
        (total, None)
    }
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        let mut data = String::new();
        if std::fs::File::open("/proc/meminfo")
            .and_then(|file| file.take(16384).read_to_string(&mut data))
            .is_ok()
        {
            parse_meminfo(&data)
        } else {
            (None, None)
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // The API initializes the provided memory status structure on success.
        if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
            (Some(status.ullTotalPhys), Some(status.ullAvailPhys))
        } else {
            (None, None)
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        (None, None)
    }
}

pub struct Memory {
    checked: std::time::Instant,
    values: (Option<u64>, Option<u64>),
    cpu_previous: Option<(u64, u64)>,
    cpu_usage: Option<f64>,
    last_sample: Option<chrono::DateTime<chrono::Utc>>,
}
impl Memory {
    pub fn new() -> Self {
        Self {
            checked: std::time::Instant::now(),
            values: memory_info(),
            cpu_previous: cpu_times(),
            cpu_usage: None,
            last_sample: None,
        }
    }
    pub fn snapshot(&mut self) -> Value {
        if self.checked.elapsed() >= std::time::Duration::from_secs(5) {
            if !cfg!(target_os = "macos") {
                self.values = memory_info();
            }
            let current = cpu_times();
            self.cpu_usage = cpu_percent(self.cpu_previous, current);
            self.cpu_previous = current;
            self.checked = std::time::Instant::now();
            self.last_sample = Some(std::time::SystemTime::now().into());
        }
        json!({"physical_memory_bytes":self.values.0,"available_memory_bytes":self.values.1,"cpu_usage_percent":self.cpu_usage,"cpu_scope":cpu_scope()})
    }
    pub fn last_sample(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.last_sample
    }
}

#[derive(Default)]
pub struct DeviceCompletion {
    fonts: bool,
    gpus: bool,
    ready_at: Option<chrono::DateTime<chrono::Utc>>,
}
impl DeviceCompletion {
    pub fn fonts_finished(&mut self) {
        self.fonts = true;
        self.mark_ready();
    }
    pub fn gpus_finished(&mut self) {
        self.gpus = true;
        self.mark_ready();
    }
    fn mark_ready(&mut self) {
        if self.fonts && self.gpus && self.ready_at.is_none() {
            self.ready_at = Some(std::time::SystemTime::now().into());
        }
    }
    pub fn ready_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.ready_at
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_cpu_times(line: &str) -> Option<(u64, u64)> {
    let mut fields = line.split_ascii_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields
        .take(8)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if values.len() < 4 {
        return None;
    }
    let idle = values[3].checked_add(values.get(4).copied().unwrap_or(0))?;
    let total = values
        .iter()
        .try_fold(0u64, |sum, value| sum.checked_add(*value))?;
    Some((idle, total))
}
#[cfg(target_os = "linux")]
fn cpu_times() -> Option<(u64, u64)> {
    use std::io::{BufRead, BufReader, Read};
    let file = std::fs::File::open("/proc/stat").ok()?;
    let mut line = String::new();
    BufReader::new(file.take(1024)).read_line(&mut line).ok()?;
    parse_cpu_times(&line)
}
#[cfg(target_os = "macos")]
fn cpu_times() -> Option<(u64, u64)> {
    extern "C" {
        fn mach_port_deallocate(task: u32, name: u32) -> i32;
        fn mach_host_self() -> u32;
        static mach_task_self_: u32;
    }
    let mut ticks = [0u32; 4];
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // The host right is released after host_statistics, including failed calls.
    unsafe {
        let host = mach_host_self();
        let status = libc::host_statistics(
            host,
            libc::HOST_CPU_LOAD_INFO,
            ticks.as_mut_ptr().cast(),
            &mut count,
        );
        let _ = mach_port_deallocate(mach_task_self_, host);
        if status != libc::KERN_SUCCESS || count != libc::HOST_CPU_LOAD_INFO_COUNT {
            return None;
        }
    }
    Some((
        u64::from(ticks[2]),
        ticks.iter().map(|value| u64::from(*value)).sum(),
    ))
}
#[cfg(windows)]
fn cpu_times() -> Option<(u64, u64)> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // GetSystemTimes initializes all three FILETIME outputs on success.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    let value =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Some((value(idle), value(kernel).checked_add(value(user))?))
}
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn cpu_times() -> Option<(u64, u64)> {
    None
}

fn cpu_scope() -> &'static str {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetActiveProcessorCount, ALL_PROCESSOR_GROUPS,
        };
        // ALL_PROCESSOR_GROUPS requests the installed logical processor count.
        if unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } > 64 {
            return "processor_group";
        }
    }
    "system"
}
fn cpu_percent(previous: Option<(u64, u64)>, current: Option<(u64, u64)>) -> Option<f64> {
    let ((old_idle, old_total), (idle, total)) = previous.zip(current)?;
    let delta_idle = idle.checked_sub(old_idle)?;
    let delta_total = total.checked_sub(old_total)?;
    if delta_total == 0 || delta_idle > delta_total {
        return None;
    }
    Some(((1.0 - delta_idle as f64 / delta_total as f64) * 1000.0).round() / 10.0)
}
