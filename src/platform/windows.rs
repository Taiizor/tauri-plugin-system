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

#[cfg(feature = "disk")]
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};

#[cfg(feature = "network")]
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GetIfEntry2, GAA_FLAG_INCLUDE_PREFIX,
    MIB_IF_ROW2,
};

#[cfg(feature = "network")]
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

#[cfg(feature = "network")]
use windows::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6,
};

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

// ─── Display DPI Helper ───

#[cfg(feature = "display")]
fn get_display_dpi() -> f64 {
    use windows::Win32::UI::HiDpi::GetDpiForSystem;

    unsafe {
        let dpi = GetDpiForSystem();
        if dpi > 0 {
            dpi as f64
        } else {
            96.0 // default DPI
        }
    }
}

// ─── GPU Registry Helper ───

#[cfg(feature = "gpu")]
fn read_registry_string_gpu(subkey: &str, value_name: &str) -> Result<String, Box<dyn StdError>> {
    use windows::Win32::System::Registry::{
        RegOpenKeyExW, RegQueryValueExW, RegCloseKey, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ, REG_VALUE_TYPE,
    };
    use windows::core::PCWSTR;

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

        let wide_slice = std::slice::from_raw_parts(
            buffer.as_ptr() as *const u16,
            data_size as usize / 2,
        );
        let len = wide_slice.iter().position(|&c| c == 0).unwrap_or(wide_slice.len());
        Ok(String::from_utf16_lossy(&wide_slice[..len]))
    }
}

// ─── SSD/HDD Detection via DeviceIoControl ───

#[cfg(feature = "disk")]
fn detect_disk_kind_for_drive(drive_path: &str) -> DiskKind {
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::Foundation::CloseHandle;

    // Extract the drive letter from a path like "C:\"
    let drive_letter = match drive_path.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => c,
        _ => return DiskKind::Unknown,
    };

    // Open the physical drive via the volume device path
    let device_path = format!("\\\\.\\{}:", drive_letter);
    let device_wide: Vec<u16> = device_path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let handle = CreateFileW(
            windows::core::PCWSTR(device_wide.as_ptr()),
            0, // No access needed for IOCTL query
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        );

        let handle = match handle {
            Ok(h) => h,
            Err(_) => return DiskKind::Unknown,
        };

        // IOCTL_STORAGE_QUERY_PROPERTY = CTL_CODE(IOCTL_STORAGE_BASE, 0x0500, METHOD_BUFFERED, FILE_ANY_ACCESS)
        // = 0x002D1400
        const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D1400;

        // StorageDeviceSeekPenaltyProperty = 7
        // PropertyStandardQuery = 0
        #[repr(C)]
        struct StoragePropertyQuery {
            property_id: u32,
            query_type: u32,
            additional_parameters: [u8; 1],
        }

        #[repr(C)]
        struct DeviceSeekPenaltyDescriptor {
            version: u32,
            size: u32,
            incurs_seek_penalty: u8, // BOOLEAN
        }

        let query = StoragePropertyQuery {
            property_id: 7, // StorageDeviceSeekPenaltyProperty
            query_type: 0,  // PropertyStandardQuery
            additional_parameters: [0],
        };

        let mut result = DeviceSeekPenaltyDescriptor {
            version: 0,
            size: 0,
            incurs_seek_penalty: 1,
        };
        let mut bytes_returned: u32 = 0;

        let ok: Result<(), _> = DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const std::ffi::c_void),
            std::mem::size_of::<StoragePropertyQuery>() as u32,
            Some(&mut result as *mut _ as *mut std::ffi::c_void),
            std::mem::size_of::<DeviceSeekPenaltyDescriptor>() as u32,
            Some(&mut bytes_returned),
            None,
        );

        let _: Result<(), _> = CloseHandle(handle);

        if ok.is_ok() && bytes_returned > 0 {
            if result.incurs_seek_penalty == 0 {
                DiskKind::Ssd
            } else {
                DiskKind::Hdd
            }
        } else {
            DiskKind::Unknown
        }
    }
}

// ─── CPU Usage via NtQuerySystemInformation ───

#[cfg(feature = "cpu")]
type CpuTimesResult = Result<Vec<(i64, i64, i64)>, Box<dyn StdError>>;

#[cfg(feature = "cpu")]
fn query_cpu_times() -> CpuTimesResult {
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
                usage.push(pct.clamp(0.0, 100.0));
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

    // ─── Disk Info ───

    #[cfg(feature = "disk")]
    fn disk_info(&self) -> Result<Vec<DiskInfo>, Box<dyn StdError>> {
        // Windows drive type constants
        const DRIVE_REMOVABLE_CONST: u32 = 2;
        const DRIVE_CDROM_CONST: u32 = 5;

        let mut disks = Vec::new();

        unsafe {
            // Get all logical drive strings (e.g., "C:\\\0D:\\\0\0")
            let mut buffer = vec![0u16; 512];
            let len = GetLogicalDriveStringsW(Some(&mut buffer));
            if len == 0 {
                return Ok(disks);
            }

            // Parse the double-null-terminated string list
            let drive_strings = &buffer[..len as usize];
            let mut start = 0;
            for (i, &ch) in drive_strings.iter().enumerate() {
                if ch == 0 {
                    if i > start {
                        let drive_wide = &drive_strings[start..=i]; // include null terminator
                        let drive_str = String::from_utf16_lossy(&drive_strings[start..i]);
                        let drive_pcwstr = windows::core::PCWSTR(drive_wide.as_ptr());

                        // Get drive type
                        let drive_type = GetDriveTypeW(drive_pcwstr);
                        let is_removable = drive_type == DRIVE_REMOVABLE_CONST || drive_type == DRIVE_CDROM_CONST;

                        // Skip DRIVE_UNKNOWN (0) or DRIVE_NO_ROOT_DIR (1)
                        if drive_type <= 1 {
                            start = i + 1;
                            continue;
                        }

                        // Get volume information (name, filesystem type)
                        let mut vol_name_buf = vec![0u16; 256];
                        let mut fs_name_buf = vec![0u16; 256];
                        let mut serial_number: u32 = 0;
                        let mut max_component_length: u32 = 0;
                        let mut fs_flags: u32 = 0;

                        let vol_ok = GetVolumeInformationW(
                            drive_pcwstr,
                            Some(&mut vol_name_buf),
                            Some(&mut serial_number),
                            Some(&mut max_component_length),
                            Some(&mut fs_flags),
                            Some(&mut fs_name_buf),
                        );

                        let vol_name = if vol_ok.is_ok() {
                            let len = vol_name_buf.iter().position(|&c| c == 0).unwrap_or(vol_name_buf.len());
                            String::from_utf16_lossy(&vol_name_buf[..len])
                        } else {
                            String::new()
                        };

                        let fs_type = if vol_ok.is_ok() {
                            let len = fs_name_buf.iter().position(|&c| c == 0).unwrap_or(fs_name_buf.len());
                            String::from_utf16_lossy(&fs_name_buf[..len])
                        } else {
                            String::new()
                        };

                        // Get disk free space
                        let mut free_bytes_available: u64 = 0;
                        let mut total_bytes: u64 = 0;
                        let mut total_free_bytes: u64 = 0;

                        let space_ok = GetDiskFreeSpaceExW(
                            drive_pcwstr,
                            Some(&mut free_bytes_available),
                            Some(&mut total_bytes),
                            Some(&mut total_free_bytes),
                        );

                        if space_ok.is_err() {
                            // Drive might not be ready (e.g. empty CD drive)
                            start = i + 1;
                            continue;
                        }

                        let used_bytes = total_bytes.saturating_sub(total_free_bytes);

                        // SSD detection: try to query seek penalty property via DeviceIoControl
                        let kind = detect_disk_kind_for_drive(&drive_str);

                        let name = if vol_name.is_empty() {
                            drive_str.trim_end_matches('\\').to_string()
                        } else {
                            vol_name
                        };

                        let mount_point = drive_str.trim_end_matches('\\').to_string();

                        disks.push(DiskInfo {
                            name,
                            mount_point,
                            fs_type,
                            kind,
                            total_bytes,
                            used_bytes,
                            free_bytes: total_free_bytes,
                            is_removable,
                        });
                    }
                    start = i + 1;
                }
            }
        }

        Ok(disks)
    }

    // ─── GPU Info via DXGI ───

    #[cfg(feature = "gpu")]
    fn gpu_info(&self) -> Result<Vec<GpuInfo>, Box<dyn StdError>> {
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
        };

        let mut gpus = Vec::new();

        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;

            let mut adapter_index: u32 = 0;
            loop {
                let adapter = match factory.EnumAdapters1(adapter_index) {
                    Ok(a) => a,
                    Err(_) => break,
                };
                adapter_index += 1;

                let desc = adapter.GetDesc1()?;

                // Skip software adapters (Microsoft Basic Render Driver)
                if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }

                // Convert wide-char description to String
                let name_len = desc.Description.iter().position(|&c| c == 0)
                    .unwrap_or(desc.Description.len());
                let name = String::from_utf16_lossy(&desc.Description[..name_len]);

                // Map VendorId to vendor name
                let vendor = match desc.VendorId {
                    0x10DE => "NVIDIA".to_string(),
                    0x1002 | 0x1022 => "AMD".to_string(),
                    0x8086 => "Intel".to_string(),
                    0x106B => "Apple".to_string(),
                    0x1414 => "Microsoft".to_string(),
                    0x5143 => "Qualcomm".to_string(),
                    other => format!("Unknown (0x{:04X})", other),
                };

                // VRAM in MB
                let vram_mb = desc.DedicatedVideoMemory as u64 / (1024 * 1024);

                // Attempt to read driver version from registry
                let driver_version = {
                    let reg_path = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
                    let mut found_version = String::new();
                    // Try subkeys 0000, 0001, 0002, etc.
                    for i in 0..16u32 {
                        let subkey = format!("{}\\{:04}", reg_path, i);
                        if let Ok(desc_reg) = read_registry_string_gpu(&subkey, "DriverDesc") {
                            // Match adapter by name similarity
                            if name.contains(&desc_reg) || desc_reg.contains(&name) || i == (adapter_index - 1) {
                                if let Ok(ver) = read_registry_string_gpu(&subkey, "DriverVersion") {
                                    found_version = ver;
                                    break;
                                }
                            }
                        }
                        // If no match by name, just try the index corresponding to adapter order
                        if found_version.is_empty() && i == (adapter_index - 1) {
                            if let Ok(ver) = read_registry_string_gpu(&subkey, "DriverVersion") {
                                found_version = ver;
                                break;
                            }
                        }
                    }
                    found_version
                };

                gpus.push(GpuInfo {
                    name,
                    vendor,
                    vram_mb,
                    driver_version,
                });
            }
        }

        Ok(gpus)
    }

    #[cfg(feature = "battery")]
    fn battery_info(&self) -> Result<Option<BatteryInfo>, Box<dyn StdError>> {
        use windows::Win32::System::Power::GetSystemPowerStatus;
        use windows::Win32::System::Power::SYSTEM_POWER_STATUS;

        unsafe {
            let mut power_status = SYSTEM_POWER_STATUS::default();
            GetSystemPowerStatus(&mut power_status)?;

            // BatteryFlag == 128 means no system battery
            if power_status.BatteryFlag == 128 {
                return Ok(None);
            }

            // BatteryLifePercent: 0-100 or 255 for unknown
            let charge_percent = if power_status.BatteryLifePercent == 255 {
                0.0
            } else {
                power_status.BatteryLifePercent as f64
            };

            // Determine battery status from ACLineStatus and BatteryFlag
            let status = if power_status.BatteryFlag & 8 != 0 {
                // Bit 3: charging
                BatteryStatus::Charging
            } else if power_status.ACLineStatus == 1 {
                // On AC but not charging
                if charge_percent >= 100.0 {
                    BatteryStatus::Full
                } else {
                    BatteryStatus::NotCharging
                }
            } else if power_status.ACLineStatus == 0 {
                BatteryStatus::Discharging
            } else {
                BatteryStatus::Unknown
            };

            // BatteryLifeTime: seconds of remaining battery life, or u32::MAX (-1 as unsigned) if unknown
            let time_to_empty_secs = if power_status.BatteryLifeTime != u32::MAX {
                Some(power_status.BatteryLifeTime as u64)
            } else {
                None
            };

            // BatteryFullLifeTime: seconds of full battery life, or u32::MAX if unknown
            let time_to_full_secs = if power_status.BatteryFullLifeTime != u32::MAX
                && status == BatteryStatus::Charging
            {
                // Estimate time to full from full life time and current charge
                if charge_percent > 0.0 && charge_percent < 100.0 {
                    let full_time = power_status.BatteryFullLifeTime as f64;
                    Some(((100.0 - charge_percent) / 100.0 * full_time) as u64)
                } else {
                    None
                }
            } else {
                None
            };

            Ok(Some(BatteryInfo {
                charge_percent,
                status,
                health_percent: None,
                cycle_count: None,
                design_capacity_mwh: None,
                full_charge_capacity_mwh: None,
                time_to_empty_secs,
                time_to_full_secs,
            }))
        }
    }

    // ─── Network Info ───

    #[cfg(feature = "network")]
    fn network_info(&self) -> Result<Vec<NetworkInfo>, Box<dyn StdError>> {
        let mut interfaces = Vec::new();

        unsafe {
            // First call to determine buffer size
            let mut buf_len: u32 = 0;
            let family = AF_UNSPEC.0 as u32;
            let flags = GAA_FLAG_INCLUDE_PREFIX;

            let ret = GetAdaptersAddresses(family, flags, None, None, &mut buf_len);
            if ret != 111 {
                // ERROR_BUFFER_OVERFLOW = 111
                return Err(format!("GetAdaptersAddresses sizing failed: {}", ret).into());
            }

            let mut buffer: Vec<u8> = vec![0u8; buf_len as usize];
            let adapter_ptr = buffer.as_mut_ptr() as *mut windows::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH;

            let ret = GetAdaptersAddresses(
                family,
                flags,
                None,
                Some(adapter_ptr),
                &mut buf_len,
            );

            if ret != 0 {
                return Err(format!("GetAdaptersAddresses failed: {}", ret).into());
            }

            let mut current = adapter_ptr;
            while !current.is_null() {
                let adapter = &*current;

                // Get friendly name
                let name = if !adapter.FriendlyName.0.is_null() {
                    let mut len = 0;
                    let mut p = adapter.FriendlyName.0;
                    while *p != 0 {
                        len += 1;
                        p = p.add(1);
                    }
                    String::from_utf16_lossy(std::slice::from_raw_parts(adapter.FriendlyName.0, len))
                } else {
                    String::new()
                };

                // Get MAC address
                let phys_len = adapter.PhysicalAddressLength as usize;
                let mac_address = if phys_len > 0 {
                    adapter.PhysicalAddress[..phys_len]
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(":")
                } else {
                    String::new()
                };

                // Walk unicast address list for IPv4/IPv6
                let mut ipv4 = Vec::new();
                let mut ipv6 = Vec::new();
                let mut unicast = adapter.FirstUnicastAddress;
                while !unicast.is_null() {
                    let ua = &*unicast;
                    let sa = ua.Address.lpSockaddr;
                    if !sa.is_null() {
                        let sa_family = (*sa).sa_family;
                        if sa_family == AF_INET {
                            let sin = &*(sa as *const SOCKADDR_IN);
                            let addr = sin.sin_addr.S_un.S_addr.to_ne_bytes();
                            ipv4.push(format!("{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3]));
                        } else if sa_family == AF_INET6 {
                            let sin6 = &*(sa as *const SOCKADDR_IN6);
                            let addr = sin6.sin6_addr.u.Byte;
                            let segments: Vec<String> = (0..8)
                                .map(|i| {
                                    let hi = addr[i * 2] as u16;
                                    let lo = addr[i * 2 + 1] as u16;
                                    format!("{:x}", (hi << 8) | lo)
                                })
                                .collect();
                            ipv6.push(segments.join(":"));
                        }
                    }
                    unicast = (*unicast).Next;
                }

                // Check operational status
                let is_up = adapter.OperStatus == IfOperStatusUp;

                // Get rx/tx bytes using GetIfEntry2
                let mut rx_bytes: u64 = 0;
                let mut tx_bytes: u64 = 0;
                let if_index = adapter.Anonymous1.Anonymous.IfIndex;
                if if_index != 0 {
                    let mut row = MIB_IF_ROW2 { InterfaceIndex: if_index, ..Default::default() };
                    if GetIfEntry2(&mut row).is_ok() {
                        rx_bytes = row.InOctets;
                        tx_bytes = row.OutOctets;
                    }
                }

                interfaces.push(NetworkInfo {
                    name,
                    mac_address,
                    ipv4,
                    ipv6,
                    rx_bytes,
                    tx_bytes,
                    is_up,
                });

                current = adapter.Next;
            }
        }

        Ok(interfaces)
    }

    #[cfg(feature = "thermal")]
    fn thermal_info(&self) -> Result<Vec<ThermalInfo>, Box<dyn StdError>> {
        // Thermal sensor access on Windows requires WMI (COM initialization,
        // MSAcpi_ThermalZoneTemperature queries) which is complex and often
        // requires administrator privileges. Return an empty Vec for now.
        // A WMI-based implementation can be added in the future.
        Ok(vec![])
    }

    #[cfg(feature = "display")]
    fn display_info(&self) -> Result<Vec<DisplayInfo>, Box<dyn StdError>> {
        use windows::Win32::Graphics::Gdi::{
            EnumDisplayDevicesW, EnumDisplaySettingsW,
            DISPLAY_DEVICEW, DEVMODEW,
            ENUM_CURRENT_SETTINGS, DISPLAY_DEVICE_ACTIVE,
            DISPLAY_DEVICE_PRIMARY_DEVICE,
        };

        let mut displays = Vec::new();

        unsafe {
            let mut device_index: u32 = 0;
            loop {
                let mut device = DISPLAY_DEVICEW {
                    cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                    ..Default::default()
                };

                let ok = EnumDisplayDevicesW(
                    None,
                    device_index,
                    &mut device,
                    0,
                );
                if !ok.as_bool() {
                    break;
                }
                device_index += 1;

                // Skip inactive devices
                if (device.StateFlags & DISPLAY_DEVICE_ACTIVE) == 0 {
                    continue;
                }

                let is_primary = (device.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE) != 0;

                // Get device name
                let name_len = device.DeviceName.iter().position(|&c| c == 0)
                    .unwrap_or(device.DeviceName.len());
                let device_name_wide = &device.DeviceName[..name_len];
                let device_name = String::from_utf16_lossy(device_name_wide);

                // Get friendly name from the monitor device (second level enumeration)
                let mut monitor_device = DISPLAY_DEVICEW {
                    cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                    ..Default::default()
                };
                let device_name_pcwstr = windows::core::PCWSTR(device.DeviceName.as_ptr());
                let friendly_name = if EnumDisplayDevicesW(
                    device_name_pcwstr,
                    0,
                    &mut monitor_device,
                    0,
                ).as_bool() {
                    let len = monitor_device.DeviceString.iter().position(|&c| c == 0)
                        .unwrap_or(monitor_device.DeviceString.len());
                    let s = String::from_utf16_lossy(&monitor_device.DeviceString[..len]);
                    if s.is_empty() { device_name.clone() } else { s }
                } else {
                    device_name.clone()
                };

                // Get current display settings (resolution, refresh rate)
                let mut devmode = DEVMODEW {
                    dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                    ..Default::default()
                };

                let settings_ok = EnumDisplaySettingsW(
                    device_name_pcwstr,
                    ENUM_CURRENT_SETTINGS,
                    &mut devmode,
                );
                if !settings_ok.as_bool() {
                    continue;
                }

                let width = devmode.dmPelsWidth;
                let height = devmode.dmPelsHeight;
                let refresh_rate = devmode.dmDisplayFrequency;
                let refresh_rate_hz = if refresh_rate > 0 && refresh_rate != 1 {
                    Some(refresh_rate as f64)
                } else {
                    None
                };

                // Get DPI using GetDpiForSystem as a fallback (per-monitor DPI requires HMONITOR)
                let dpi = get_display_dpi();

                displays.push(DisplayInfo {
                    name: friendly_name,
                    width,
                    height,
                    dpi,
                    refresh_rate_hz,
                    is_primary,
                });
            }
        }

        Ok(displays)
    }
}
