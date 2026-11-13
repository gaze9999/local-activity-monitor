//! Windows DXGI adapter metadata, with bounded SetupAPI physical-device counts.
use crate::gpu_info::{pci_identity, AdapterFilter};
use serde_json::{json, Value};
use std::{collections::BTreeMap, ffi::c_void};
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
            SetupDiGetDeviceInstanceIdW, DIGCF_PRESENT, SP_DEVINFO_DATA,
        },
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
    },
};

struct DeviceSet(isize);
impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
fn physical_counts() -> Option<BTreeMap<(u32, u32, u32), usize>> {
    let class = GUID::from_u128(0x4d36e968_e325_11ce_bfc1_08002be10318);
    // Present display-class devices only; the handle is released by DeviceSet.
    let handle = unsafe {
        SetupDiGetClassDevsW(
            &class,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT,
        )
    };
    if handle == 0 || handle == -1 {
        return None;
    }
    let handle = DeviceSet(handle);
    let mut counts = BTreeMap::new();
    for index in 0..64 {
        let mut info = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInfo(handle.0, index, &mut info) } == 0 {
            break;
        }
        let mut buffer = [0u16; 512];
        if unsafe {
            SetupDiGetDeviceInstanceIdW(
                handle.0,
                &info,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            continue;
        }
        let length = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        if let Ok(identity) = String::from_utf16(&buffer[..length]) {
            if let Some(id) = pci_identity(&identity) {
                *counts.entry(id).or_insert(0) += 1;
            }
        }
    }
    Some(counts)
}
struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
struct Com(*mut c_void);
impl Com {
    unsafe fn method(&self, index: usize) -> *const () {
        let vtable = *(self.0 as *const *const *const ());
        *vtable.add(index)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            let release: unsafe extern "system" fn(*mut c_void) -> u32 =
                std::mem::transmute(self.method(2));
            release(self.0);
        }
    }
}
#[repr(C)]
struct AdapterDescription {
    name: [u16; 128],
    vendor: u32,
    device: u32,
    subsystem: u32,
    revision: u32,
    dedicated: usize,
    system: usize,
    shared: usize,
    luid: [u32; 2],
    flags: u32,
}

pub fn collect() -> Vec<Value> {
    let counts = physical_counts();
    let dll: Vec<u16> = "dxgi.dll\0".encode_utf16().collect();
    // Restrict loading to the system directory, as in the Python implementation.
    let handle = unsafe {
        LoadLibraryExW(
            dll.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if handle.is_null() {
        return Vec::new();
    }
    let library = Library(handle);
    let Some(address) = (unsafe { GetProcAddress(library.0, b"CreateDXGIFactory1\0".as_ptr()) })
    else {
        return Vec::new();
    };
    type Create = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> i32;
    let create: Create = unsafe { std::mem::transmute(address) };
    let factory_id = GUID::from_u128(0x770aae78_f26f_4dba_a829_253c83d1b387);
    let driver_id = GUID::from_u128(0x54ec77fa_1377_44e6_8c32_88fd5f44c84c);
    let mut factory = std::ptr::null_mut();
    let create_status = unsafe { create(&factory_id, &mut factory) };
    if factory.is_null() {
        return Vec::new();
    }
    let factory = Com(factory);
    if create_status != 0 {
        return Vec::new();
    }
    let enumerate: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 =
        unsafe { std::mem::transmute(factory.method(12)) };
    let mut selected = AdapterFilter::default();
    let mut result = Vec::new();
    for index in 0..16 {
        let mut adapter = std::ptr::null_mut();
        let status = unsafe { enumerate(factory.0, index, &mut adapter) };
        if adapter.is_null() {
            break;
        }
        let adapter = Com(adapter);
        if status != 0 {
            break;
        }
        let describe: unsafe extern "system" fn(*mut c_void, *mut AdapterDescription) -> i32 =
            unsafe { std::mem::transmute(adapter.method(10)) };
        let mut description: AdapterDescription = unsafe { std::mem::zeroed() };
        if unsafe { describe(adapter.0, &mut description) } != 0
            || !selected.accept(
                description.flags,
                (description.luid[0], description.luid[1]),
                (
                    description.vendor,
                    description.device,
                    description.subsystem,
                ),
                counts.as_ref(),
            )
        {
            continue;
        }
        let version_query: unsafe extern "system" fn(*mut c_void, *const GUID, *mut i64) -> i32 =
            unsafe { std::mem::transmute(adapter.method(9)) };
        let mut raw_version = 0i64;
        let driver = if unsafe { version_query(adapter.0, &driver_id, &mut raw_version) } == 0 {
            let version = raw_version as u64;
            Some(
                [48, 32, 16, 0]
                    .map(|shift| ((version >> shift) & 0xffff).to_string())
                    .join("."),
            )
        } else {
            None
        };
        let length = description
            .name
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(description.name.len());
        let name = String::from_utf16_lossy(&description.name[..length])
            .trim()
            .to_owned();
        result.push(json!({"name":if name.is_empty(){None}else{Some(name)},"dedicated_memory_bytes":if std::mem::size_of::<usize>()==8 {Some(description.dedicated)}else{None},"shared_memory_bytes":if std::mem::size_of::<usize>()==8 {Some(description.shared)}else{None},"driver_version":driver}));
    }
    result
}
