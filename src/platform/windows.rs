use crate::models::*;
use crate::platform::SystemInfoProvider;
use std::error::Error as StdError;

#[cfg(any(feature = "os", feature = "cpu"))]
use windows::Win32::System::SystemInformation::{
    GetNativeSystemInfo, SYSTEM_INFO,
};

#[cfg(feature = "os")]
use windows::Win32::System::SystemInformation::{
    GetComputerNameExW, GetTickCount64, ComputerNameDnsHostname,
};

#[cfg(feature = "os")]
use windows::Wdk::System::SystemServices::RtlGetVersion;

#[cfg(feature = "os")]
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

#[cfg(feature = "os")]
use windows::core::PWSTR;

#[cfg(feature = "cpu")]
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

#[cfg(feature = "cpu")]
use windows::Win32::System::Registry::{
    RegOpenKeyExW, RegQueryValueExW, RegCloseKey, HKEY, HKEY_LOCAL_MACHINE,
    KEY_READ, REG_VALUE_TYPE,
};

#[cfg(feature = "cpu")]
use windows::core::PCWSTR;

#[cfg(feature = "memory")]
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

pub struct WindowsSystemInfo;

impl WindowsSystemInfo {
    pub fn new() -> Self {
        WindowsSystemInfo
    }
}

// ─── Helper functions ───

#[cfg(any(feature = "os", feature = "cpu"))]
fn get_native_system_info() -> SYSTEM_INFO {
    let mut sys_info = SYSTEM_INFO::default();
    unsafe {
        GetNativeSystemInfo(&mut sys_info);
    }
    sys_info
}

#[cfg(any(feature = "os", feature = "cpu"))]
fn get_arch_string(sys_info: &SYSTEM_INFO) -> String {
    // wProcessorArchitecture is inside the Anonymous union
    let arch = unsafe { sys_info.Anonymous.Anonymous.wProcessorArchitecture };
    match arch.0 {
        9 => "x86_64".to_string(),   // PROCESSOR_ARCHITECTURE_AMD64
        12 => "aarch64".to_string(),  // PROCESSOR_ARCHITECTURE_ARM64
        5 => "arm".to_string(),       // PROCESSOR_ARCHITECTURE_ARM
        0 => "x86".to_string(),       // PROCESSOR_ARCHITECTURE_INTEL
        6 => "ia64".to_string(),      // PROCESSOR_ARCHITECTURE_IA64
        _ => "unknown".to_string(),
    }
}

#[cfg(feature = "cpu")]
fn read_registry_string(subkey: &str, value_name: &str) -> Result<String, Box<dyn StdError>> {
    unsafe {
        let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let value_wide: Vec<u16> = value_name.encode_utf16().chain(std::iter::once(0)).collect();

        let mut hkey = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey_wide.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );

        if result.is_err() {
            return Err(format!("Failed to open registry key: {}", subkey).into());
        }

        // First call to get the size
        let mut data_type = REG_VALUE_TYPE::default();
        let mut data_size: u32 = 0;
        let result = RegQueryValueExW(
            hkey,
            PCWSTR(value_wide.as_ptr()),
            None,
            Some(&mut data_type),
            None,
            Some(&mut data_size),
        );

        if result.is_err() {
            let _ = RegCloseKey(hkey);
            return Err(format!("Failed to query registry value size: {}", value_name).into());
        }

        // Allocate buffer and read value
        let mut buffer: Vec<u8> = vec![0u8; data_size as usize];
        let result = RegQueryValueExW(
            hkey,
            PCWSTR(value_wide.as_ptr()),
            None,
            Some(&mut data_type),
            Some(buffer.as_mut_ptr()),
            Some(&mut data_size),
        );

        let _ = RegCloseKey(hkey);

        if result.is_err() {
            return Err(format!("Failed to read registry value: {}", value_name).into());
        }

        // Convert wide string to Rust String
        let wide_slice = std::slice::from_raw_parts(
            buffer.as_ptr() as *const u16,
            data_size as usize / 2,
        );
        // Trim null terminator
        let len = wide_slice.iter().position(|&c| c == 0).unwrap_or(wide_slice.len());
        Ok(String::from_utf16_lossy(&wide_slice[..len]))
    }
}

#[cfg(feature = "cpu")]
fn read_registry_dword(subkey: &str, value_name: &str) -> Result<u32, Box<dyn StdError>> {
    unsafe {
        let subkey_wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let value_wide: Vec<u16> = value_name.encode_utf16().chain(std::iter::once(0)).collect();

        let mut hkey = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey_wide.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );

        if result.is_err() {
            return Err(format!("Failed to open registry key: {}", subkey).into());
        }

        let mut data_type = REG_VALUE_TYPE::default();
        let mut data: u32 = 0;
        let mut data_size: u32 = std::mem::size_of::<u32>() as u32;
        let result = RegQueryValueExW(
            hkey,
            PCWSTR(value_wide.as_ptr()),
            None,
            Some(&mut data_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_size),
        );

        let _ = RegCloseKey(hkey);

        if result.is_err() {
            return Err(format!("Failed to read registry DWORD: {}", value_name).into());
        }

        Ok(data)
    }
}

#[cfg(feature = "cpu")]
fn get_physical_core_count() -> Result<u32, Box<dyn StdError>> {
    unsafe {
        // First call to get required buffer size
        let mut length: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            None,
            &mut length,
        );

        if length == 0 {
            return Err("GetLogicalProcessorInformationEx returned zero length".into());
        }

        // Allocate buffer
        let mut buffer: Vec<u8> = vec![0u8; length as usize];
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buffer.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut length,
        )?;

        // Count entries - each entry represents one physical core
        let mut core_count: u32 = 0;
        let mut offset: usize = 0;
        while offset < length as usize {
            let info = &*(buffer.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            core_count += 1;
            offset += info.Size as usize;
        }

        Ok(core_count)
    }
}

// ─── CPU Usage via NtQuerySystemInformation ───

#[cfg(feature = "cpu")]
fn query_cpu_times() -> Result<Vec<(i64, i64, i64)>, Box<dyn StdError>> {
    use windows::Win32::System::WindowsProgramming::SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION;
    use windows::Wdk::System::SystemInformation::NtQuerySystemInformation;
    use windows::Wdk::System::SystemInformation::SYSTEM_INFORMATION_CLASS;

    let sys_info = get_native_system_info();
    let num_cpus = sys_info.dwNumberOfProcessors as usize;

    let struct_size = std::mem::size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>();
    let buffer_size = struct_size * num_cpus;
    let mut buffer: Vec<u8> = vec![0u8; buffer_size];
    let mut return_length: u32 = 0;

    // SystemProcessorPerformanceInformation = 8
    let status = unsafe {
        NtQuerySystemInformation(
            SYSTEM_INFORMATION_CLASS(8),
            buffer.as_mut_ptr() as *mut std::ffi::c_void,
            buffer_size as u32,
            &mut return_length,
        )
    };

    if status.is_err() {
        return Err(format!("NtQuerySystemInformation failed: {:?}", status).into());
    }

    let entries = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr() as *const SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
            num_cpus,
        )
    };

    let mut times = Vec::with_capacity(num_cpus);
    for entry in entries {
        times.push((entry.IdleTime, entry.KernelTime, entry.UserTime));
    }

    Ok(times)
}

// ─── SystemInfoProvider Implementation ───

impl SystemInfoProvider for WindowsSystemInfo {
    #[cfg(feature = "os")]
    fn os_info(&self) -> Result<OsInfo, Box<dyn StdError>> {
        // --- OS Version via RtlGetVersion ---
        let (name, version, full_version) = unsafe {
            let mut os_info = OSVERSIONINFOW {
                dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
                ..Default::default()
            };

            let status = RtlGetVersion(&mut os_info);
            if status.is_err() {
                return Err("RtlGetVersion failed".into());
            }

            let major = os_info.dwMajorVersion;
            let minor = os_info.dwMinorVersion;
            let build = os_info.dwBuildNumber;

            let name = if major >= 10 {
                if build >= 22000 {
                    "Windows 11".to_string()
                } else {
                    "Windows 10".to_string()
                }
            } else if major == 6 {
                match minor {
                    3 => "Windows 8.1".to_string(),
                    2 => "Windows 8".to_string(),
                    1 => "Windows 7".to_string(),
                    0 => "Windows Vista".to_string(),
                    _ => format!("Windows {}.{}", major, minor),
                }
            } else {
                format!("Windows {}.{}", major, minor)
            };

            let version = format!("{}.{}.{}", major, minor, build);
            let full_version = format!("{} (Build {})", name, build);

            (name, version, full_version)
        };

        // --- Hostname ---
        let hostname = unsafe {
            // First call to get required size
            let mut size: u32 = 0;
            let _ = GetComputerNameExW(ComputerNameDnsHostname, PWSTR::null(), &mut size);

            let mut buffer: Vec<u16> = vec![0u16; size as usize];
            GetComputerNameExW(
                ComputerNameDnsHostname,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            )?;

            String::from_utf16_lossy(&buffer[..size as usize])
        };

        // --- Architecture ---
        let sys_info = get_native_system_info();
        let arch = get_arch_string(&sys_info);

        // --- Uptime ---
        let uptime_secs = unsafe { GetTickCount64() / 1000 };

        // --- Username ---
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "unknown".to_string());

        Ok(OsInfo {
            name,
            version,
            full_version,
            hostname,
            arch,
            uptime_secs,
            username,
        })
    }

    #[cfg(feature = "cpu")]
    fn cpu_info(&self) -> Result<CpuInfo, Box<dyn StdError>> {
        let reg_path = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";

        // Read model name
        let model = read_registry_string(reg_path, "ProcessorNameString")
            .unwrap_or_else(|_| "Unknown CPU".to_string())
            .trim()
            .to_string();

        // Read vendor
        let vendor = read_registry_string(reg_path, "VendorIdentifier")
            .unwrap_or_else(|_| "Unknown".to_string())
            .trim()
            .to_string();

        // Read frequency
        let frequency_mhz = read_registry_dword(reg_path, "~MHz")
            .unwrap_or(0) as u64;

        // Get logical processor count
        let sys_info = get_native_system_info();
        let threads = sys_info.dwNumberOfProcessors;

        // Get physical core count
        let cores = get_physical_core_count().unwrap_or(threads);

        // Architecture
        let arch = get_arch_string(&sys_info);

        Ok(CpuInfo {
            model,
            vendor,
            cores,
            threads,
            arch,
            frequency_mhz,
        })
    }

    #[cfg(feature = "cpu")]
    fn cpu_usage(&self) -> Result<Vec<f64>, Box<dyn StdError>> {
        // Take first sample
        let times1 = query_cpu_times()?;

        // Sleep 100ms
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Take second sample
        let times2 = query_cpu_times()?;

        let mut usage = Vec::with_capacity(times1.len());
        for i in 0..times1.len() {
            let (idle1, kernel1, user1) = times1[i];
            let (idle2, kernel2, user2) = times2[i];

            let idle_delta = (idle2 - idle1) as f64;
            let kernel_delta = (kernel2 - kernel1) as f64;
            let user_delta = (user2 - user1) as f64;

            // KernelTime includes IdleTime, so total = kernel + user
            let total = kernel_delta + user_delta;
            if total > 0.0 {
                // Active time = total - idle
                let active = total - idle_delta;
                let pct = (active / total) * 100.0;
                // Clamp to [0.0, 100.0]
                usage.push(pct.max(0.0).min(100.0));
            } else {
                usage.push(0.0);
            }
        }

        Ok(usage)
    }

    #[cfg(feature = "memory")]
    fn memory_info(&self) -> Result<MemoryInfo, Box<dyn StdError>> {
        unsafe {
            let mut mem_status = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };

            GlobalMemoryStatusEx(&mut mem_status)?;

            let total = mem_status.ullTotalPhys;
            let available = mem_status.ullAvailPhys;
            let used = total.saturating_sub(available);

            let page_total = mem_status.ullTotalPageFile;
            let page_avail = mem_status.ullAvailPageFile;

            // Swap = page file minus physical memory
            let swap_total = page_total.saturating_sub(total);
            let page_used = page_total.saturating_sub(page_avail);
            let phys_used = used;
            let swap_used = page_used.saturating_sub(phys_used);

            Ok(MemoryInfo {
                total_bytes: total,
                used_bytes: used,
                free_bytes: available,
                available_bytes: available,
                swap_total_bytes: swap_total,
                swap_used_bytes: swap_used,
            })
        }
    }

    // ─── Stubs for unimplemented features ───

    #[cfg(feature = "disk")]
    fn disk_info(&self) -> Result<Vec<DiskInfo>, Box<dyn StdError>> {
        Err("Disk info not yet implemented".into())
    }

    #[cfg(feature = "gpu")]
    fn gpu_info(&self) -> Result<Vec<GpuInfo>, Box<dyn StdError>> {
        Err("GPU info not yet implemented".into())
    }

    #[cfg(feature = "battery")]
    fn battery_info(&self) -> Result<Option<BatteryInfo>, Box<dyn StdError>> {
        Err("Battery info not yet implemented".into())
    }

    #[cfg(feature = "network")]
    fn network_info(&self) -> Result<Vec<NetworkInfo>, Box<dyn StdError>> {
        Err("Network info not yet implemented".into())
    }

    #[cfg(feature = "thermal")]
    fn thermal_info(&self) -> Result<Vec<ThermalInfo>, Box<dyn StdError>> {
        Err("Thermal info not yet implemented".into())
    }

    #[cfg(feature = "display")]
    fn display_info(&self) -> Result<Vec<DisplayInfo>, Box<dyn StdError>> {
        Err("Display info not yet implemented".into())
    }
}
