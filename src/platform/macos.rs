use crate::models::*;
use crate::platform::SystemInfoProvider;
use std::error::Error as StdError;

pub struct MacOSSystemInfo;

impl MacOSSystemInfo {
    pub fn new() -> Self {
        Self
    }
}

// ─── Sysctl helper functions ───

/// Read a sysctl value as a String (for string-type sysctls like kern.hostname).
#[cfg(any(feature = "os", feature = "cpu"))]
fn sysctl_string(name: &str) -> Result<String, Box<dyn StdError>> {
    use std::ffi::CString;

    let c_name = CString::new(name)?;
    let mut size: libc::size_t = 0;

    // First call to get buffer size
    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(format!(
            "sysctlbyname size query failed for '{}': errno {}",
            name, ret
        )
        .into());
    }
    if size == 0 {
        return Err(format!("sysctlbyname returned zero size for '{}'", name).into());
    }

    let mut buf: Vec<u8> = vec![0u8; size];
    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(format!("sysctlbyname read failed for '{}': errno {}", name, ret).into());
    }

    // Strip trailing null bytes
    while buf.last() == Some(&0) {
        buf.pop();
    }

    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Read a sysctl value as a u64 (works for both 32-bit and 64-bit integer sysctls).
#[cfg(any(feature = "os", feature = "cpu", feature = "memory"))]
fn sysctl_u64(name: &str) -> Result<u64, Box<dyn StdError>> {
    use std::ffi::CString;

    let c_name = CString::new(name)?;
    let mut size: libc::size_t = 0;

    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(format!(
            "sysctlbyname size query failed for '{}': errno {}",
            name, ret
        )
        .into());
    }

    match size {
        4 => {
            let mut val: u32 = 0;
            let mut val_size = std::mem::size_of::<u32>();
            let ret = unsafe {
                libc::sysctlbyname(
                    c_name.as_ptr(),
                    &mut val as *mut u32 as *mut libc::c_void,
                    &mut val_size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if ret != 0 {
                return Err(
                    format!("sysctlbyname read failed for '{}': errno {}", name, ret).into(),
                );
            }
            Ok(val as u64)
        }
        8 => {
            let mut val: u64 = 0;
            let mut val_size = std::mem::size_of::<u64>();
            let ret = unsafe {
                libc::sysctlbyname(
                    c_name.as_ptr(),
                    &mut val as *mut u64 as *mut libc::c_void,
                    &mut val_size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if ret != 0 {
                return Err(
                    format!("sysctlbyname read failed for '{}': errno {}", name, ret).into(),
                );
            }
            Ok(val)
        }
        other => Err(format!("Unexpected sysctl size {} for '{}'", other, name).into()),
    }
}

/// Read kern.boottime as a timeval struct and return the boot timestamp in seconds.
#[cfg(feature = "os")]
fn sysctl_boottime() -> Result<libc::timeval, Box<dyn StdError>> {
    use std::ffi::CString;

    let c_name = CString::new("kern.boottime")?;
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();

    let ret = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            &mut tv as *mut libc::timeval as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(format!("sysctlbyname failed for kern.boottime: errno {}", ret).into());
    }

    Ok(tv)
}

// ─── CPU Usage via Mach host_processor_info ───

#[cfg(feature = "cpu")]
mod cpu_usage_impl {
    use std::error::Error as StdError;

    // Mach types and constants
    // host_processor_info returns per-CPU load ticks in CPU_STATE_MAX slots.
    pub const CPU_STATE_USER: usize = 0;
    pub const CPU_STATE_SYSTEM: usize = 1;
    pub const CPU_STATE_IDLE: usize = 2;
    pub const CPU_STATE_NICE: usize = 3;
    pub const CPU_STATE_MAX: usize = 4;

    // HOST_CPU_LOAD_INFO flavor for host_processor_info
    pub const PROCESSOR_CPU_LOAD_INFO: i32 = 2;

    extern "C" {
        fn mach_host_self() -> u32;
        fn host_processor_info(
            host: u32,
            flavor: i32,
            out_processor_count: *mut u32,
            out_processor_info: *mut *mut i32,
            out_processor_info_cnt: *mut u32,
        ) -> i32;
        fn vm_deallocate(target_task: u32, address: usize, size: usize) -> i32;
        fn mach_task_self() -> u32;
    }

    /// Represents one snapshot of per-CPU load ticks.
    pub struct CpuLoadSnapshot {
        pub per_cpu: Vec<[u32; CPU_STATE_MAX]>,
    }

    /// Sample the per-CPU load ticks using host_processor_info.
    pub fn sample_cpu_load() -> Result<CpuLoadSnapshot, Box<dyn StdError>> {
        let mut processor_count: u32 = 0;
        let mut info_array: *mut i32 = std::ptr::null_mut();
        let mut info_count: u32 = 0;

        let host = unsafe { mach_host_self() };
        let kr = unsafe {
            host_processor_info(
                host,
                PROCESSOR_CPU_LOAD_INFO,
                &mut processor_count,
                &mut info_array,
                &mut info_count,
            )
        };

        if kr != 0 {
            return Err(format!("host_processor_info failed with kern_return {}", kr).into());
        }

        let num_cpus = processor_count as usize;
        let mut per_cpu = Vec::with_capacity(num_cpus);

        for i in 0..num_cpus {
            let base = i * CPU_STATE_MAX;
            let ticks: [u32; CPU_STATE_MAX] = unsafe {
                [
                    *info_array.add(base + CPU_STATE_USER) as u32,
                    *info_array.add(base + CPU_STATE_SYSTEM) as u32,
                    *info_array.add(base + CPU_STATE_IDLE) as u32,
                    *info_array.add(base + CPU_STATE_NICE) as u32,
                ]
            };
            per_cpu.push(ticks);
        }

        // Deallocate the info array returned by Mach
        unsafe {
            vm_deallocate(
                mach_task_self(),
                info_array as usize,
                (info_count as usize) * std::mem::size_of::<i32>(),
            );
        }

        Ok(CpuLoadSnapshot { per_cpu })
    }
}

// ─── Memory info via Mach vm_statistics64 ───

#[cfg(feature = "memory")]
mod memory_impl {
    use std::error::Error as StdError;

    // vm_statistics64 struct (matching the Mach kernel definition)
    #[repr(C)]
    #[derive(Default)]
    pub struct VMStatistics64 {
        pub free_count: u32,
        pub active_count: u32,
        pub inactive_count: u32,
        pub wire_count: u32,
        pub zero_fill_count: u64,
        pub reactivations: u64,
        pub pageins: u64,
        pub pageouts: u64,
        pub faults: u64,
        pub cow_faults: u64,
        pub lookups: u64,
        pub hits: u64,
        pub purges: u64,
        pub purgeable_count: u32,
        pub speculative_count: u32,
        pub decompressions: u64,
        pub compressions: u64,
        pub swapins: u64,
        pub swapouts: u64,
        pub compressor_page_count: u32,
        pub throttled_count: u32,
        pub external_page_count: u32,
        pub internal_page_count: u32,
        pub total_uncompressed_pages_in_compressor: u64,
    }

    // HOST_VM_INFO64 flavor = 4
    pub const HOST_VM_INFO64: i32 = 4;
    pub const HOST_VM_INFO64_COUNT: u32 =
        (std::mem::size_of::<VMStatistics64>() / std::mem::size_of::<i32>()) as u32;

    extern "C" {
        fn mach_host_self() -> u32;
        fn host_statistics64(
            host_priv: u32,
            flavor: i32,
            host_info_out: *mut VMStatistics64,
            host_info_out_cnt: *mut u32,
        ) -> i32;
        fn host_page_size(host: u32, out_page_size: *mut u32) -> i32;
    }

    /// Swap usage struct matching macOS xsw_usage
    #[repr(C)]
    #[derive(Default)]
    pub struct XswUsage {
        pub xsu_total: u64,
        pub xsu_avail: u64,
        pub xsu_used: u64,
        pub xsu_pagesize: u32,
        pub xsu_encrypted: bool,
    }

    #[allow(dead_code)]
    pub struct MemoryStats {
        pub free_count: u64,
        pub active_count: u64,
        pub inactive_count: u64,
        pub wire_count: u64,
        pub purgeable_count: u64,
        pub speculative_count: u64,
        pub compressor_page_count: u64,
        pub page_size: u64,
    }

    pub fn get_vm_statistics() -> Result<MemoryStats, Box<dyn StdError>> {
        let host = unsafe { mach_host_self() };

        // Get page size
        let mut page_size: u32 = 0;
        let kr = unsafe { host_page_size(host, &mut page_size) };
        if kr != 0 {
            return Err(format!("host_page_size failed: kern_return {}", kr).into());
        }

        // Get vm statistics
        let mut vm_stat = VMStatistics64::default();
        let mut count = HOST_VM_INFO64_COUNT;
        let kr = unsafe { host_statistics64(host, HOST_VM_INFO64, &mut vm_stat, &mut count) };
        if kr != 0 {
            return Err(format!("host_statistics64 failed: kern_return {}", kr).into());
        }

        Ok(MemoryStats {
            free_count: vm_stat.free_count as u64,
            active_count: vm_stat.active_count as u64,
            inactive_count: vm_stat.inactive_count as u64,
            wire_count: vm_stat.wire_count as u64,
            purgeable_count: vm_stat.purgeable_count as u64,
            speculative_count: vm_stat.speculative_count as u64,
            compressor_page_count: vm_stat.compressor_page_count as u64,
            page_size: page_size as u64,
        })
    }

    pub fn get_swap_usage() -> Result<(u64, u64), Box<dyn StdError>> {
        use std::ffi::CString;

        let c_name = CString::new("vm.swapusage")?;
        let mut usage = XswUsage::default();
        let mut size = std::mem::size_of::<XswUsage>();

        let ret = unsafe {
            libc::sysctlbyname(
                c_name.as_ptr(),
                &mut usage as *mut XswUsage as *mut libc::c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret != 0 {
            return Err(format!("sysctlbyname failed for vm.swapusage: errno {}", ret).into());
        }

        Ok((usage.xsu_total, usage.xsu_used))
    }
}

// ─── Display Resolution Parser ───

/// Parse resolution string like "3456 x 2234 @ 120 Hz" or "1920 x 1080"
#[cfg(feature = "display")]
fn parse_macos_resolution(s: &str) -> (u32, u32, Option<f64>) {
    // Remove Retina markers and extra info in parentheses
    let clean = s.split('(').next().unwrap_or(s).trim();

    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut refresh: Option<f64> = None;

    // Split on " x " to get width and remainder
    if let Some((w_str, remainder)) = clean.split_once(" x ") {
        width = w_str.trim().parse().unwrap_or(0);

        // Remainder might be "2234 @ 120 Hz" or just "2234"
        if let Some((h_str, freq_part)) = remainder.split_once('@') {
            height = h_str.trim().parse().unwrap_or(0);
            // freq_part is like " 120 Hz"
            let freq_str = freq_part.trim().trim_end_matches("Hz").trim();
            if let Ok(hz) = freq_str.parse::<f64>() {
                refresh = Some(hz);
            }
        } else {
            height = remainder.trim().parse().unwrap_or(0);
        }
    }

    (width, height, refresh)
}

// ─── SystemInfoProvider Implementation ───

impl SystemInfoProvider for MacOSSystemInfo {
    #[cfg(feature = "os")]
    fn os_info(&self) -> Result<OsInfo, Box<dyn StdError>> {
        // OS version (e.g., "14.2.1")
        let version =
            sysctl_string("kern.osproductversion").unwrap_or_else(|_| "unknown".to_string());

        // Build number (e.g., "23C71")
        let build = sysctl_string("kern.osversion").unwrap_or_else(|_| "unknown".to_string());

        // Full version string (e.g., "macOS 14.2.1 (23C71)")
        let full_version = format!("macOS {} ({})", version, build);

        // Hostname
        let hostname = sysctl_string("kern.hostname").unwrap_or_else(|_| "unknown".to_string());

        // Architecture
        let arch = std::env::consts::ARCH.to_string();

        // Uptime: current time minus boot time
        let uptime_secs = match sysctl_boottime() {
            Ok(tv) => {
                let now = unsafe { libc::time(std::ptr::null_mut()) };
                let boot = tv.tv_sec;
                if now > boot {
                    (now - boot) as u64
                } else {
                    0
                }
            }
            Err(_) => 0,
        };

        // Username
        let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());

        Ok(OsInfo {
            name: "macOS".to_string(),
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
        // CPU model (e.g., "Apple M1 Pro" or "Intel(R) Core(TM) i9-9880H")
        let model =
            sysctl_string("machdep.cpu.brand_string").unwrap_or_else(|_| "Unknown CPU".to_string());

        // CPU vendor (e.g., "GenuineIntel" or "Apple")
        // On Apple Silicon, machdep.cpu.vendor may not exist
        let vendor = sysctl_string("machdep.cpu.vendor").unwrap_or_else(|_| {
            // Fallback: if the model contains "Apple", use "Apple"
            if model.contains("Apple") {
                "Apple".to_string()
            } else {
                "Unknown".to_string()
            }
        });

        // Physical cores
        let cores = sysctl_u64("hw.physicalcpu").unwrap_or(0) as u32;

        // Logical cores (threads)
        let threads = sysctl_u64("hw.logicalcpu").unwrap_or(0) as u32;

        // Architecture
        let arch = std::env::consts::ARCH.to_string();

        // CPU frequency in MHz
        // hw.cpufrequency returns Hz; divide by 1,000,000 to get MHz
        // On Apple Silicon this sysctl may not exist, so fall back to 0
        let frequency_mhz = sysctl_u64("hw.cpufrequency")
            .map(|hz| hz / 1_000_000)
            .unwrap_or(0);

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
        use cpu_usage_impl::*;

        // Take first sample
        let snap1 = sample_cpu_load()?;

        // Sleep 100ms between samples
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Take second sample
        let snap2 = sample_cpu_load()?;

        let num_cpus = snap1.per_cpu.len().min(snap2.per_cpu.len());
        let mut usage = Vec::with_capacity(num_cpus);

        for i in 0..num_cpus {
            let t1 = &snap1.per_cpu[i];
            let t2 = &snap2.per_cpu[i];

            let user_delta = t2[CPU_STATE_USER].wrapping_sub(t1[CPU_STATE_USER]) as f64;
            let system_delta = t2[CPU_STATE_SYSTEM].wrapping_sub(t1[CPU_STATE_SYSTEM]) as f64;
            let idle_delta = t2[CPU_STATE_IDLE].wrapping_sub(t1[CPU_STATE_IDLE]) as f64;
            let nice_delta = t2[CPU_STATE_NICE].wrapping_sub(t1[CPU_STATE_NICE]) as f64;

            let total = user_delta + system_delta + idle_delta + nice_delta;
            if total > 0.0 {
                let active = user_delta + system_delta + nice_delta;
                let pct = (active / total) * 100.0;
                usage.push(pct.clamp(0.0, 100.0));
            } else {
                usage.push(0.0);
            }
        }

        Ok(usage)
    }

    #[cfg(feature = "memory")]
    fn memory_info(&self) -> Result<MemoryInfo, Box<dyn StdError>> {
        // Total physical memory
        let total_bytes = sysctl_u64("hw.memsize")?;

        // Get VM statistics from Mach
        let stats = memory_impl::get_vm_statistics()?;
        let page_size = stats.page_size;

        // Available = free + inactive (pages that can be reclaimed)
        // This matches what macOS reports as "available" memory
        let free_pages = stats.free_count + stats.speculative_count;
        let available_bytes =
            (free_pages + stats.inactive_count + stats.purgeable_count) * page_size;

        // Free = just the truly free pages (free_count * page_size)
        let free_bytes = free_pages * page_size;

        // Used = total - available
        let used_bytes = total_bytes.saturating_sub(available_bytes);

        // Swap info
        let (swap_total_bytes, swap_used_bytes) = memory_impl::get_swap_usage().unwrap_or((0, 0));

        Ok(MemoryInfo {
            total_bytes,
            used_bytes,
            free_bytes,
            available_bytes,
            swap_total_bytes,
            swap_used_bytes,
        })
    }

    // ─── Disk Info ───

    #[cfg(feature = "disk")]
    fn disk_info(&self) -> Result<Vec<DiskInfo>, Box<dyn StdError>> {
        let mut disks = Vec::new();

        unsafe {
            // Get the number of mounted filesystems
            let mut mntbuf: *mut libc::statfs = std::ptr::null_mut();
            let count = libc::getmntinfo(&mut mntbuf, libc::MNT_NOWAIT);
            if count <= 0 || mntbuf.is_null() {
                return Ok(disks);
            }

            let entries = std::slice::from_raw_parts(mntbuf, count as usize);

            // Pseudo-filesystem types to skip
            let pseudo_fs = [
                "devfs", "autofs", "nullfs", "vmhgfs", "ctfs", "fdescfs", "nfs", "devpts", "tmpfs",
            ];

            for entry in entries {
                let fs_type = std::ffi::CStr::from_ptr(entry.f_fstypename.as_ptr())
                    .to_string_lossy()
                    .to_string();

                // Skip pseudo-filesystems
                if pseudo_fs.contains(&fs_type.as_str()) {
                    continue;
                }

                let device_name = std::ffi::CStr::from_ptr(entry.f_mntfromname.as_ptr())
                    .to_string_lossy()
                    .to_string();

                let mount_point = std::ffi::CStr::from_ptr(entry.f_mntonname.as_ptr())
                    .to_string_lossy()
                    .to_string();

                // Skip entries that don't start with /dev/
                if !device_name.starts_with("/dev/") {
                    continue;
                }

                #[allow(clippy::unnecessary_cast)]
                let block_size = entry.f_bsize as u64;
                #[allow(clippy::unnecessary_cast)]
                let total_bytes = entry.f_blocks as u64 * block_size;
                #[allow(clippy::unnecessary_cast)]
                let free_bytes = entry.f_bfree as u64 * block_size;
                let used_bytes = total_bytes.saturating_sub(free_bytes);

                // Use volume name from device path or mount point
                let name = if mount_point == "/" {
                    "Macintosh HD".to_string()
                } else {
                    mount_point
                        .rsplit('/')
                        .next()
                        .unwrap_or(&mount_point)
                        .to_string()
                };

                // Determine if removable based on filesystem type
                let is_removable = matches!(fs_type.as_str(), "msdos" | "exfat" | "udf");

                // SSD detection is complex on macOS (requires IOKit); default to Unknown
                let kind = DiskKind::Unknown;

                disks.push(DiskInfo {
                    name,
                    mount_point,
                    fs_type,
                    kind,
                    total_bytes,
                    used_bytes,
                    free_bytes,
                    is_removable,
                });
            }
        }

        Ok(disks)
    }

    // ─── GPU Info via system_profiler ───

    #[cfg(feature = "gpu")]
    fn gpu_info(&self) -> Result<Vec<GpuInfo>, Box<dyn StdError>> {
        use std::process::Command;

        let output = Command::new("system_profiler")
            .args(["SPDisplaysDataType", "-json"])
            .output()
            .map_err(|e| format!("Failed to run system_profiler: {}", e))?;

        if !output.status.success() {
            return Err("system_profiler returned non-zero exit code".into());
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse system_profiler JSON: {}", e))?;

        let mut gpus = Vec::new();

        if let Some(displays_array) = parsed.get("SPDisplaysDataType").and_then(|v| v.as_array()) {
            for gpu_entry in displays_array {
                // GPU name
                let name = gpu_entry
                    .get("sppci_model")
                    .or_else(|| gpu_entry.get("_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown GPU")
                    .to_string();

                // Vendor
                let vendor_raw = gpu_entry
                    .get("sppci_vendor")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let vendor = if !vendor_raw.is_empty() {
                    // system_profiler returns strings like "sppci_vendor_apple", "sppci_vendor_amd", etc.
                    if vendor_raw.contains("apple") || name.contains("Apple") {
                        "Apple".to_string()
                    } else if vendor_raw.contains("amd")
                        || vendor_raw.contains("ATI")
                        || name.contains("AMD")
                        || name.contains("Radeon")
                    {
                        "AMD".to_string()
                    } else if vendor_raw.contains("nvidia")
                        || name.contains("NVIDIA")
                        || name.contains("GeForce")
                    {
                        "NVIDIA".to_string()
                    } else if vendor_raw.contains("intel") || name.contains("Intel") {
                        "Intel".to_string()
                    } else {
                        vendor_raw.to_string()
                    }
                } else {
                    // Derive vendor from name
                    if name.contains("Apple") {
                        "Apple".to_string()
                    } else if name.contains("AMD") || name.contains("Radeon") {
                        "AMD".to_string()
                    } else if name.contains("NVIDIA") || name.contains("GeForce") {
                        "NVIDIA".to_string()
                    } else if name.contains("Intel") {
                        "Intel".to_string()
                    } else {
                        "Unknown".to_string()
                    }
                };

                // VRAM in MB
                let vram_mb = gpu_entry
                    .get("sppci_vram")
                    .or_else(|| gpu_entry.get("_spdisplays_vram"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| {
                        // Format like "8 GB" or "1536 MB"
                        let parts: Vec<&str> = s.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let amount: u64 = parts[0].parse().ok()?;
                            let unit = parts[1].to_uppercase();
                            if unit.starts_with("GB") {
                                Some(amount * 1024)
                            } else {
                                // MB or any other unit, assume MB
                                Some(amount)
                            }
                        } else {
                            // If just a number, assume MB
                            s.parse::<u64>().ok()
                        }
                    })
                    .unwrap_or(0);

                // Driver version: on Apple Silicon there's no meaningful driver version,
                // use the Metal/GPU family support or leave empty
                let driver_version = gpu_entry
                    .get("spdisplays_metal")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

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
        use std::process::Command;

        let output = Command::new("system_profiler")
            .args(["SPPowerDataType", "-json"])
            .output()
            .map_err(|e| format!("Failed to run system_profiler: {}", e))?;

        if !output.status.success() {
            return Err("system_profiler returned non-zero exit code".into());
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse system_profiler JSON: {}", e))?;

        let power_array = match parsed.get("SPPowerDataType").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return Ok(None), // No power data means no battery
        };

        // Find battery charge info and health info within the power data
        let mut charge_percent: Option<f64> = None;
        let mut is_charging = false;
        let mut is_connected = false;
        let mut cycle_count: Option<u32> = None;
        let mut health_percent: Option<f64> = None;
        let mut max_capacity: Option<u64> = None;
        let mut design_capacity: Option<u64> = None;
        let mut has_battery = false;

        for entry in power_array {
            // Check for battery charge info section
            if let Some(charge_info) = entry.get("sppower_battery_charge_info") {
                has_battery = true;

                // Current charge level
                if let Some(current) = charge_info
                    .get("sppower_battery_current_capacity")
                    .and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    })
                {
                    if let Some(max) =
                        charge_info
                            .get("sppower_battery_max_capacity")
                            .and_then(|v| {
                                v.as_u64()
                                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                            })
                    {
                        max_capacity = Some(max);
                        if max > 0 {
                            charge_percent = Some((current as f64 / max as f64) * 100.0);
                        }
                    }
                }

                // Charging state
                if let Some(charging) = charge_info.get("sppower_battery_is_charging") {
                    is_charging =
                        charging.as_str() == Some("TRUE") || charging.as_bool() == Some(true);
                }
            }

            // Check for battery health info section
            if let Some(health_info) = entry.get("sppower_battery_health_info") {
                has_battery = true;

                // Cycle count
                if let Some(cycles) = health_info
                    .get("sppower_battery_cycle_count")
                    .and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    })
                {
                    cycle_count = Some(cycles as u32);
                }

                // Maximum capacity percent (battery health)
                if let Some(max_pct) = health_info
                    .get("sppower_battery_health_maximum_capacity")
                    .and_then(|v| {
                        v.as_str()
                            .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
                            .or_else(|| v.as_f64())
                            .or_else(|| v.as_u64().map(|n| n as f64))
                    })
                {
                    health_percent = Some(max_pct);
                }
            }

            // Check for AC charger info (connected state)
            if let Some(ac_info) = entry.get("sppower_ac_charger_information") {
                if let Some(connected) = ac_info.get("sppower_battery_charger_connected") {
                    is_connected =
                        connected.as_str() == Some("TRUE") || connected.as_bool() == Some(true);
                }
            }

            // Alternative: top-level fields in some macOS versions
            if entry.get("sppower_battery_model_info").is_some() {
                has_battery = true;
            }

            // Try to get design capacity from battery model info
            if let Some(model_info) = entry.get("sppower_battery_model_info") {
                if let Some(dc) = model_info
                    .get("sppower_battery_design_capacity")
                    .and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    })
                {
                    design_capacity = Some(dc);
                }
            }
        }

        if !has_battery {
            return Ok(None);
        }

        let status = if is_charging {
            BatteryStatus::Charging
        } else if is_connected {
            if charge_percent.unwrap_or(0.0) >= 100.0 {
                BatteryStatus::Full
            } else {
                BatteryStatus::NotCharging
            }
        } else {
            BatteryStatus::Discharging
        };

        // Convert capacities to mWh (system_profiler reports in mAh typically;
        // without voltage info we store the raw value as an approximation)
        let design_capacity_mwh = design_capacity;
        let full_charge_capacity_mwh = max_capacity;

        Ok(Some(BatteryInfo {
            charge_percent: charge_percent.unwrap_or(0.0),
            status,
            health_percent,
            cycle_count,
            design_capacity_mwh,
            full_charge_capacity_mwh,
            time_to_empty_secs: None, // system_profiler doesn't reliably provide this
            time_to_full_secs: None,
        }))
    }

    // ─── Network Info ───

    #[cfg(feature = "network")]
    fn network_info(&self) -> Result<Vec<NetworkInfo>, Box<dyn StdError>> {
        use std::collections::HashMap;

        let mut iface_map: HashMap<String, NetworkInfo> = HashMap::new();

        unsafe {
            let mut ifaddrs: *mut libc::ifaddrs = std::ptr::null_mut();
            if libc::getifaddrs(&mut ifaddrs) != 0 {
                return Err("getifaddrs failed".into());
            }

            let mut current = ifaddrs;
            while !current.is_null() {
                let ifa = &*current;
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                    .to_string_lossy()
                    .to_string();

                #[allow(clippy::unnecessary_cast)]
                let iface_is_up = (ifa.ifa_flags & libc::IFF_UP as u32) != 0;
                let entry = iface_map
                    .entry(name.clone())
                    .or_insert_with(|| NetworkInfo {
                        name: name.clone(),
                        mac_address: String::new(),
                        ipv4: Vec::new(),
                        ipv6: Vec::new(),
                        rx_bytes: 0,
                        tx_bytes: 0,
                        is_up: iface_is_up,
                    });

                if !ifa.ifa_addr.is_null() {
                    let sa_family = (*ifa.ifa_addr).sa_family;

                    if sa_family == libc::AF_INET as u8 {
                        // IPv4
                        let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                        let addr_bytes = sin.sin_addr.s_addr.to_ne_bytes();
                        entry.ipv4.push(format!(
                            "{}.{}.{}.{}",
                            addr_bytes[0], addr_bytes[1], addr_bytes[2], addr_bytes[3]
                        ));
                    } else if sa_family == libc::AF_INET6 as u8 {
                        // IPv6
                        let sin6 = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                        let addr = sin6.sin6_addr.s6_addr;
                        let segments: Vec<String> = (0..8)
                            .map(|i| {
                                let hi = addr[i * 2] as u16;
                                let lo = addr[i * 2 + 1] as u16;
                                format!("{:x}", (hi << 8) | lo)
                            })
                            .collect();
                        entry.ipv6.push(segments.join(":"));
                    } else if sa_family == libc::AF_LINK as u8 {
                        // AF_LINK: MAC address and traffic counters
                        // sockaddr_dl layout on macOS
                        #[repr(C)]
                        struct SockaddrDl {
                            sdl_len: u8,
                            sdl_family: u8,
                            sdl_index: u16,
                            sdl_type: u8,
                            sdl_nlen: u8,
                            sdl_alen: u8,
                            sdl_slen: u8,
                            sdl_data: [u8; 12],
                        }

                        let sdl = &*(ifa.ifa_addr as *const SockaddrDl);
                        if sdl.sdl_alen == 6 {
                            let mac_start = sdl.sdl_nlen as usize;
                            let mac_bytes = &sdl.sdl_data[mac_start..mac_start + 6];
                            entry.mac_address = mac_bytes
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(":");
                        }

                        // Get rx/tx bytes from ifa_data (struct if_data)
                        if !ifa.ifa_data.is_null() {
                            // struct if_data on macOS has ifi_ibytes and ifi_obytes
                            // The layout: we only care about ifi_ibytes at offset ~112
                            // and ifi_obytes at offset ~120 (for 64-bit macOS).
                            // Simpler approach: define the struct partially.
                            #[repr(C)]
                            struct IfData {
                                ifi_type: u8,
                                ifi_typelen: u8,
                                ifi_physical: u8,
                                ifi_addrlen: u8,
                                ifi_hdrlen: u8,
                                ifi_recvquota: u8,
                                ifi_xmitquota: u8,
                                ifi_unused1: u8,
                                ifi_mtu: u32,
                                ifi_metric: u32,
                                ifi_baudrate: u32,
                                ifi_ipackets: u32,
                                ifi_ierrors: u32,
                                ifi_opackets: u32,
                                ifi_oerrors: u32,
                                ifi_collisions: u32,
                                ifi_ibytes: u32,
                                ifi_obytes: u32,
                                // ... more fields follow
                            }

                            let if_data = &*(ifa.ifa_data as *const IfData);
                            entry.rx_bytes = if_data.ifi_ibytes as u64;
                            entry.tx_bytes = if_data.ifi_obytes as u64;
                        }
                    }
                }

                current = ifa.ifa_next;
            }

            libc::freeifaddrs(ifaddrs);
        }

        Ok(iface_map.into_values().collect())
    }

    #[cfg(feature = "thermal")]
    fn thermal_info(&self) -> Result<Vec<ThermalInfo>, Box<dyn StdError>> {
        // Thermal sensor access on macOS requires IOKit SMC (AppleSMC) which
        // involves privileged access to the System Management Controller via
        // IOKit framework calls (SMCReadKey). This is complex and often
        // requires elevated privileges. Return an empty Vec for now.
        // An IOKit SMC-based implementation can be added in the future.
        Ok(vec![])
    }

    #[cfg(feature = "display")]
    fn display_info(&self) -> Result<Vec<DisplayInfo>, Box<dyn StdError>> {
        use std::process::Command;

        // Use system_profiler to get display info in JSON format
        let output = Command::new("system_profiler")
            .args(["SPDisplaysDataType", "-json"])
            .output()
            .map_err(|e| format!("Failed to run system_profiler: {}", e))?;

        if !output.status.success() {
            return Err("system_profiler returned non-zero exit code".into());
        }

        let json_str = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse system_profiler JSON: {}", e))?;

        let mut displays = Vec::new();
        let mut is_first_display = true;

        if let Some(gpu_array) = parsed.get("SPDisplaysDataType").and_then(|v| v.as_array()) {
            for gpu_entry in gpu_array {
                // Each GPU entry may have a "spdisplays_ndrvs" array of connected displays
                if let Some(display_array) =
                    gpu_entry.get("spdisplays_ndrvs").and_then(|v| v.as_array())
                {
                    for display_entry in display_array {
                        let name = display_entry
                            .get("_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown Display")
                            .to_string();

                        // Resolution: "_spdisplays_resolution" like "3456 x 2234 @ 120 Hz"
                        // or "_spdisplays_pixels" like "3456 x 2234"
                        let (width, height, refresh_rate_hz) = display_entry
                            .get("_spdisplays_resolution")
                            .and_then(|v| v.as_str())
                            .map(parse_macos_resolution)
                            .unwrap_or((0, 0, None));

                        // DPI: try to get from pixel resolution and physical size
                        // system_profiler provides "_spdisplays_resolution" but not always physical size
                        // Use Retina flag to estimate DPI
                        let is_retina = display_entry
                            .get("spdisplays_retina")
                            .and_then(|v| v.as_str())
                            .map(|s| s.contains("spdisplays_yes"))
                            .unwrap_or(false);
                        let dpi = if is_retina { 144.0 } else { 72.0 };

                        // Primary: the main display is typically the first one listed
                        let is_main = display_entry
                            .get("spdisplays_main")
                            .and_then(|v| v.as_str())
                            .map(|s| s.contains("spdisplays_yes"))
                            .unwrap_or(false);
                        let is_primary = is_main || (is_first_display && displays.is_empty());

                        if is_first_display {
                            is_first_display = false;
                        }

                        if width > 0 && height > 0 {
                            displays.push(DisplayInfo {
                                name,
                                width,
                                height,
                                dpi,
                                refresh_rate_hz,
                                is_primary,
                            });
                        }
                    }
                }
            }
        }

        Ok(displays)
    }
}
