use crate::models::*;
use crate::platform::SystemInfoProvider;
use std::error::Error as StdError;

#[cfg(feature = "cpu")]
use std::collections::HashSet;

pub struct LinuxSystemInfo;

impl LinuxSystemInfo {
    pub fn new() -> Self {
        Self
    }
}

// ─── Helper: Parse /etc/os-release ───

#[cfg(feature = "os")]
fn parse_os_release() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                // Strip surrounding quotes from value
                let value = value.trim_matches('"').trim_matches('\'').to_string();
                map.insert(key.to_string(), value);
            }
        }
    }
    map
}

// ─── Helper: Parse /proc/cpuinfo ───

#[cfg(feature = "cpu")]
struct CpuInfoRaw {
    model: String,
    vendor: String,
    cores: u32,
    threads: u32,
    frequency_mhz: u64,
}

#[cfg(feature = "cpu")]
fn parse_proc_cpuinfo() -> Result<CpuInfoRaw, Box<dyn StdError>> {
    let content = std::fs::read_to_string("/proc/cpuinfo")
        .map_err(|e| format!("Failed to read /proc/cpuinfo: {}", e))?;

    let mut model = String::new();
    let mut vendor = String::new();
    let mut frequency_mhz: u64 = 0;
    let mut threads: u32 = 0;
    let mut core_ids: HashSet<String> = HashSet::new();
    let mut cpu_cores_field: Option<u32> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "model name" => {
                    if model.is_empty() {
                        model = value.to_string();
                    }
                }
                "vendor_id" => {
                    if vendor.is_empty() {
                        vendor = value.to_string();
                    }
                }
                "cpu MHz" => {
                    if frequency_mhz == 0 {
                        frequency_mhz = value
                            .parse::<f64>()
                            .map(|v| v as u64)
                            .unwrap_or(0);
                    }
                }
                "processor" => {
                    threads += 1;
                }
                "core id" => {
                    core_ids.insert(value.to_string());
                }
                "cpu cores" => {
                    if cpu_cores_field.is_none() {
                        cpu_cores_field = value.parse::<u32>().ok();
                    }
                }
                _ => {}
            }
        }
    }

    // Determine physical core count: prefer unique core ids, fallback to cpu cores field,
    // then fallback to thread count
    let cores = if !core_ids.is_empty() {
        core_ids.len() as u32
    } else if let Some(c) = cpu_cores_field {
        c
    } else {
        threads
    };

    // If frequency_mhz is still 0, try sysfs
    if frequency_mhz == 0 {
        if let Ok(freq_str) =
            std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        {
            if let Ok(khz) = freq_str.trim().parse::<u64>() {
                frequency_mhz = khz / 1000;
            }
        }
    }

    Ok(CpuInfoRaw {
        model,
        vendor,
        cores,
        threads,
        frequency_mhz,
    })
}

// ─── Helper: Parse /proc/stat for per-CPU times ───

#[cfg(feature = "cpu")]
struct CpuTimes {
    total: u64,
    idle_total: u64,
}

#[cfg(feature = "cpu")]
fn parse_proc_stat() -> Result<Vec<CpuTimes>, Box<dyn StdError>> {
    let content = std::fs::read_to_string("/proc/stat")
        .map_err(|e| format!("Failed to read /proc/stat: {}", e))?;

    let mut cpu_times = Vec::new();

    for line in content.lines() {
        // Match lines like "cpu0 ...", "cpu1 ...", etc. — skip the aggregate "cpu " line
        if line.starts_with("cpu") && !line.starts_with("cpu ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Fields: cpuN user nice system idle iowait irq softirq steal [guest guest_nice]
            if parts.len() < 8 {
                continue;
            }

            let user: u64 = parts[1].parse().unwrap_or(0);
            let nice: u64 = parts[2].parse().unwrap_or(0);
            let system: u64 = parts[3].parse().unwrap_or(0);
            let idle: u64 = parts[4].parse().unwrap_or(0);
            let iowait: u64 = parts[5].parse().unwrap_or(0);
            let irq: u64 = parts[6].parse().unwrap_or(0);
            let softirq: u64 = parts[7].parse().unwrap_or(0);
            let steal: u64 = if parts.len() > 8 {
                parts[8].parse().unwrap_or(0)
            } else {
                0
            };

            let total = user + nice + system + idle + iowait + irq + softirq + steal;
            let idle_total = idle + iowait;

            cpu_times.push(CpuTimes { total, idle_total });
        }
    }

    Ok(cpu_times)
}

// ─── Helper: Parse /proc/meminfo ───

#[cfg(feature = "memory")]
fn parse_proc_meminfo() -> Result<std::collections::HashMap<String, u64>, Box<dyn StdError>> {
    let content = std::fs::read_to_string("/proc/meminfo")
        .map_err(|e| format!("Failed to read /proc/meminfo: {}", e))?;

    let mut map = std::collections::HashMap::new();

    for line in content.lines() {
        // Lines look like: "MemTotal:       16384000 kB"
        if let Some((key, rest)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value_str = rest.trim();
            // Strip the " kB" suffix if present, then parse the number
            let numeric_part = value_str
                .split_whitespace()
                .next()
                .unwrap_or("0");
            if let Ok(value_kb) = numeric_part.parse::<u64>() {
                // Values in /proc/meminfo labeled "kB" are actually KiB (1024 bytes)
                map.insert(key, value_kb * 1024);
            }
        }
    }

    Ok(map)
}

// ─── SystemInfoProvider Implementation ───

impl SystemInfoProvider for LinuxSystemInfo {
    #[cfg(feature = "os")]
    fn os_info(&self) -> Result<OsInfo, Box<dyn StdError>> {
        let os_release = parse_os_release();

        // Name: prefer PRETTY_NAME, then NAME, fallback to "Linux"
        let name = os_release
            .get("PRETTY_NAME")
            .or_else(|| os_release.get("NAME"))
            .cloned()
            .unwrap_or_else(|| "Linux".to_string());

        // Version: VERSION_ID field
        let version = os_release
            .get("VERSION_ID")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        // Full version: combine NAME + VERSION, or fallback to uname -r via /proc/version
        let full_version = {
            let os_name = os_release
                .get("NAME")
                .cloned()
                .unwrap_or_else(|| "Linux".to_string());
            let os_version = os_release
                .get("VERSION")
                .cloned();

            if let Some(ver) = os_version {
                format!("{} {}", os_name, ver)
            } else if let Ok(proc_version) = std::fs::read_to_string("/proc/version") {
                // /proc/version contains something like "Linux version 5.15.0-generic ..."
                // Extract the kernel version
                let kernel_ver = proc_version
                    .split_whitespace()
                    .nth(2)
                    .unwrap_or("unknown");
                format!("{} (kernel {})", os_name, kernel_ver)
            } else {
                os_name
            }
        };

        // Hostname
        let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        // Architecture
        let arch = std::env::consts::ARCH.to_string();

        // Uptime: first number in /proc/uptime is seconds as float
        let uptime_secs = std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|content| {
                content
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v as u64)
            })
            .unwrap_or(0);

        // Username
        let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());

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
        let raw = parse_proc_cpuinfo()?;

        Ok(CpuInfo {
            model: if raw.model.is_empty() {
                "Unknown CPU".to_string()
            } else {
                raw.model
            },
            vendor: if raw.vendor.is_empty() {
                "Unknown".to_string()
            } else {
                raw.vendor
            },
            cores: raw.cores,
            threads: raw.threads,
            arch: std::env::consts::ARCH.to_string(),
            frequency_mhz: raw.frequency_mhz,
        })
    }

    #[cfg(feature = "cpu")]
    fn cpu_usage(&self) -> Result<Vec<f64>, Box<dyn StdError>> {
        // First sample
        let times1 = parse_proc_stat()?;

        // Sleep 100ms between samples
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Second sample
        let times2 = parse_proc_stat()?;

        if times1.len() != times2.len() {
            return Err("CPU count changed between samples".into());
        }

        let mut usage = Vec::with_capacity(times1.len());
        for i in 0..times1.len() {
            let total_delta = times2[i].total.saturating_sub(times1[i].total) as f64;
            let idle_delta = times2[i].idle_total.saturating_sub(times1[i].idle_total) as f64;

            if total_delta > 0.0 {
                let pct = (1.0 - idle_delta / total_delta) * 100.0;
                usage.push(pct.max(0.0).min(100.0));
            } else {
                usage.push(0.0);
            }
        }

        Ok(usage)
    }

    #[cfg(feature = "memory")]
    fn memory_info(&self) -> Result<MemoryInfo, Box<dyn StdError>> {
        let meminfo = parse_proc_meminfo()?;

        let total_bytes = *meminfo.get("MemTotal").unwrap_or(&0);
        let free_bytes = *meminfo.get("MemFree").unwrap_or(&0);
        let available_bytes = *meminfo.get("MemAvailable").unwrap_or(&free_bytes);
        let used_bytes = total_bytes.saturating_sub(available_bytes);

        let swap_total_bytes = *meminfo.get("SwapTotal").unwrap_or(&0);
        let swap_free_bytes = *meminfo.get("SwapFree").unwrap_or(&0);
        let swap_used_bytes = swap_total_bytes.saturating_sub(swap_free_bytes);

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
        let mounts = std::fs::read_to_string("/proc/mounts")
            .map_err(|e| format!("Failed to read /proc/mounts: {}", e))?;

        // Pseudo-filesystem types to skip
        let pseudo_fs = [
            "proc", "sysfs", "devtmpfs", "tmpfs", "cgroup", "cgroup2",
            "pstore", "debugfs", "securityfs", "configfs", "fusectl",
            "mqueue", "hugetlbfs", "devpts", "autofs", "binfmt_misc",
            "tracefs", "efivarfs", "bpf", "overlay", "nsfs", "ramfs",
            "rpc_pipefs", "nfsd", "fuse.portal", "fuse.gvfsd-fuse",
        ];

        let mut disks = Vec::new();

        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }

            let device = parts[0];
            let mount_point = parts[1];
            let fs_type = parts[2];

            // Skip pseudo-filesystems
            if pseudo_fs.contains(&fs_type) {
                continue;
            }

            // Only include real block devices that start with /dev/
            if !device.starts_with("/dev/") {
                continue;
            }

            // Get disk space using statvfs
            let mount_c = std::ffi::CString::new(mount_point)?;
            let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
            let ret = unsafe { libc::statvfs(mount_c.as_ptr(), &mut stat) };
            if ret != 0 {
                continue;
            }

            let block_size = stat.f_frsize as u64;
            let total_bytes = stat.f_blocks as u64 * block_size;
            let free_bytes = stat.f_bfree as u64 * block_size;
            let used_bytes = total_bytes.saturating_sub(free_bytes);

            // Skip entries with 0 total bytes (virtual entries)
            if total_bytes == 0 {
                continue;
            }

            // Extract base device name for sysfs lookups (e.g., /dev/sda1 -> sda)
            let dev_name = device
                .rsplit('/')
                .next()
                .unwrap_or(device);
            // Strip partition number to get base device (sda1 -> sda, nvme0n1p1 -> nvme0n1)
            let base_device = if dev_name.starts_with("nvme") || dev_name.starts_with("mmcblk") {
                // NVMe: nvme0n1p1 -> nvme0n1, MMC: mmcblk0p1 -> mmcblk0
                if let Some(pos) = dev_name.rfind('p') {
                    if dev_name[pos + 1..].chars().all(|c| c.is_ascii_digit()) {
                        &dev_name[..pos]
                    } else {
                        dev_name
                    }
                } else {
                    dev_name
                }
            } else {
                // SCSI/SATA: sda1 -> sda, vda1 -> vda
                dev_name.trim_end_matches(|c: char| c.is_ascii_digit())
            };

            // Detect SSD vs HDD
            let kind = {
                let rotational_path = format!("/sys/block/{}/queue/rotational", base_device);
                match std::fs::read_to_string(&rotational_path) {
                    Ok(val) => match val.trim() {
                        "0" => DiskKind::Ssd,
                        "1" => DiskKind::Hdd,
                        _ => DiskKind::Unknown,
                    },
                    Err(_) => DiskKind::Unknown,
                }
            };

            // Check if removable
            let is_removable = {
                let removable_path = format!("/sys/block/{}/removable", base_device);
                match std::fs::read_to_string(&removable_path) {
                    Ok(val) => val.trim() == "1",
                    Err(_) => false,
                }
            };

            // Use device basename as name
            let name = dev_name.to_string();

            disks.push(DiskInfo {
                name,
                mount_point: mount_point.to_string(),
                fs_type: fs_type.to_string(),
                kind,
                total_bytes,
                used_bytes,
                free_bytes,
                is_removable,
            });
        }

        Ok(disks)
    }

    // ─── Stubs for unimplemented features ───

    #[cfg(feature = "gpu")]
    fn gpu_info(&self) -> Result<Vec<GpuInfo>, Box<dyn StdError>> {
        Err("GPU info not yet implemented".into())
    }

    #[cfg(feature = "battery")]
    fn battery_info(&self) -> Result<Option<BatteryInfo>, Box<dyn StdError>> {
        Err("Battery info not yet implemented".into())
    }

    // ─── Network Info ───

    #[cfg(feature = "network")]
    fn network_info(&self) -> Result<Vec<NetworkInfo>, Box<dyn StdError>> {
        use std::collections::HashMap;

        let mut iface_map: HashMap<String, NetworkInfo> = HashMap::new();

        // Parse /proc/net/dev for rx/tx byte counters
        if let Ok(content) = std::fs::read_to_string("/proc/net/dev") {
            for line in content.lines().skip(2) {
                // Lines look like: "  eth0: 1234 ... 5678 ..."
                let line = line.trim();
                if let Some((name, rest)) = line.split_once(':') {
                    let name = name.trim().to_string();
                    let values: Vec<u64> = rest
                        .split_whitespace()
                        .filter_map(|v| v.parse().ok())
                        .collect();

                    // Fields: rx_bytes rx_packets ... tx_bytes tx_packets ...
                    let rx_bytes = values.first().copied().unwrap_or(0);
                    let tx_bytes = values.get(8).copied().unwrap_or(0);

                    iface_map.insert(name.clone(), NetworkInfo {
                        name,
                        mac_address: String::new(),
                        ipv4: Vec::new(),
                        ipv6: Vec::new(),
                        rx_bytes,
                        tx_bytes,
                        is_up: false,
                    });
                }
            }
        }

        // Read MAC addresses from /sys/class/net/<iface>/address
        for entry in iface_map.values_mut() {
            let mac_path = format!("/sys/class/net/{}/address", entry.name);
            if let Ok(mac) = std::fs::read_to_string(&mac_path) {
                entry.mac_address = mac.trim().to_string();
            }

            // Read operational state from /sys/class/net/<iface>/operstate
            let state_path = format!("/sys/class/net/{}/operstate", entry.name);
            if let Ok(state) = std::fs::read_to_string(&state_path) {
                entry.is_up = state.trim() == "up";
            }
        }

        // Get IP addresses using getifaddrs
        unsafe {
            let mut ifaddrs: *mut libc::ifaddrs = std::ptr::null_mut();
            if libc::getifaddrs(&mut ifaddrs) == 0 {
                let mut current = ifaddrs;
                while !current.is_null() {
                    let ifa = &*current;
                    let name = std::ffi::CStr::from_ptr(ifa.ifa_name)
                        .to_string_lossy()
                        .to_string();

                    if !ifa.ifa_addr.is_null() {
                        let sa_family = (*ifa.ifa_addr).sa_family;

                        // Ensure we have an entry for this interface
                        let entry = iface_map.entry(name.clone()).or_insert_with(|| NetworkInfo {
                            name: name.clone(),
                            mac_address: String::new(),
                            ipv4: Vec::new(),
                            ipv6: Vec::new(),
                            rx_bytes: 0,
                            tx_bytes: 0,
                            is_up: (ifa.ifa_flags as u32 & libc::IFF_UP as u32) != 0,
                        });

                        if sa_family == libc::AF_INET as u16 {
                            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                            let addr_bytes = sin.sin_addr.s_addr.to_ne_bytes();
                            entry.ipv4.push(format!(
                                "{}.{}.{}.{}",
                                addr_bytes[0], addr_bytes[1], addr_bytes[2], addr_bytes[3]
                            ));
                        } else if sa_family == libc::AF_INET6 as u16 {
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
                        }
                    }

                    current = ifa.ifa_next;
                }

                libc::freeifaddrs(ifaddrs);
            }
        }

        Ok(iface_map.into_values().collect())
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
