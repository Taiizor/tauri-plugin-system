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

// ─── GPU VRAM Helper for NVIDIA ───

#[cfg(feature = "gpu")]
fn nvidia_vram_from_proc(_card_num: &str) -> Option<u64> {
    // Try /proc/driver/nvidia/gpus/*/information
    if let Ok(entries) = std::fs::read_dir("/proc/driver/nvidia/gpus") {
        for entry in entries.filter_map(|e| e.ok()) {
            let info_path = entry.path().join("information");
            if let Ok(content) = std::fs::read_to_string(&info_path) {
                for line in content.lines() {
                    if line.starts_with("Video BIOS") {
                        continue;
                    }
                    if line.contains("Memory") || line.contains("FB Size") {
                        // Look for lines like "FB Size:    8192 MB"
                        if let Some((_, val_part)) = line.split_once(':') {
                            let val_str = val_part.trim();
                            let parts: Vec<&str> = val_str.split_whitespace().collect();
                            if let Some(num_str) = parts.first() {
                                if let Ok(mb) = num_str.parse::<u64>() {
                                    return Some(mb);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

// ─── Display Info xrandr Parser Helpers ───

#[cfg(feature = "display")]
fn parse_xrandr_displays() -> Result<Vec<crate::models::DisplayInfo>, Box<dyn std::error::Error>> {
    use std::process::Command;

    let output = Command::new("xrandr")
        .arg("--query")
        .output()
        .map_err(|e| format!("Failed to run xrandr: {}", e))?;

    if !output.status.success() {
        return Err("xrandr returned non-zero exit code".into());
    }

    let xrandr_output = String::from_utf8_lossy(&output.stdout);
    let mut displays = Vec::new();

    // Parse xrandr output line by line
    // Connected output lines look like:
    //   eDP-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 344mm x 193mm
    //   HDMI-1 connected 3840x2160+1920+0 (normal left inverted right x axis y axis) 600mm x 340mm
    // Mode lines follow with resolution and refresh rate:
    //   1920x1080     60.00*+  59.97    59.96    48.00
    //   3840x2160     60.00*+

    let mut current_name = String::new();
    let mut current_is_primary = false;
    let mut current_width_mm: f64 = 0.0;
    let mut current_width: u32 = 0;
    let mut current_height: u32 = 0;
    let mut found_active_mode = false;
    let mut in_connected_output = false;

    for line in xrandr_output.lines() {
        if line.contains(" connected") {
            // Save previous display if we were tracking one
            if in_connected_output && current_width > 0 && current_height > 0 {
                // This shouldn't happen if we properly capture modes, but just in case
            }

            in_connected_output = true;
            found_active_mode = false;
            current_is_primary = line.contains(" primary ");

            // Extract monitor name (first word)
            current_name = line.split_whitespace().next().unwrap_or("Unknown").to_string();

            // Extract physical size in mm (e.g., "344mm x 193mm")
            current_width_mm = 0.0;
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if part.ends_with("mm") && i + 2 < parts.len() && parts[i + 1] == "x" && parts[i + 2].ends_with("mm") {
                    current_width_mm = part.trim_end_matches("mm").parse().unwrap_or(0.0);
                    break;
                }
            }

            // Extract resolution from the connected line (e.g., "1920x1080+0+0")
            current_width = 0;
            current_height = 0;
            for part in &parts {
                if part.contains('x') && part.contains('+') {
                    // Format: WIDTHxHEIGHT+X+Y
                    if let Some(res_part) = part.split('+').next() {
                        if let Some((w, h)) = res_part.split_once('x') {
                            current_width = w.parse().unwrap_or(0);
                            current_height = h.parse().unwrap_or(0);
                        }
                    }
                    break;
                }
            }
        } else if line.contains(" disconnected") {
            in_connected_output = false;
        } else if in_connected_output && !found_active_mode && line.contains('*') {
            // This is the active mode line, e.g., "  1920x1080     60.00*+  59.97"
            found_active_mode = true;
            let trimmed = line.trim();

            // Parse resolution from mode line if we didn't get it from the connected line
            if current_width == 0 || current_height == 0 {
                if let Some(res_str) = trimmed.split_whitespace().next() {
                    if let Some((w, h)) = res_str.split_once('x') {
                        current_width = w.parse().unwrap_or(0);
                        current_height = h.parse().unwrap_or(0);
                    }
                }
            }

            // Find the refresh rate marked with '*'
            let mut refresh_rate_hz: Option<f64> = None;
            for token in trimmed.split_whitespace().skip(1) {
                if token.contains('*') {
                    let rate_str = token.replace('*', "").replace('+', "");
                    if let Ok(hz) = rate_str.parse::<f64>() {
                        refresh_rate_hz = Some(hz);
                    }
                    break;
                }
            }

            // Calculate DPI from physical size
            let dpi = if current_width_mm > 0.0 && current_width > 0 {
                (current_width as f64) / (current_width_mm / 25.4)
            } else {
                96.0 // default DPI
            };

            if current_width > 0 && current_height > 0 {
                displays.push(crate::models::DisplayInfo {
                    name: current_name.clone(),
                    width: current_width,
                    height: current_height,
                    dpi,
                    refresh_rate_hz,
                    is_primary: current_is_primary,
                });
            }

            in_connected_output = false; // Done with this output
        }
    }

    Ok(displays)
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

    // ─── GPU Info via sysfs + lspci ───

    #[cfg(feature = "gpu")]
    fn gpu_info(&self) -> Result<Vec<GpuInfo>, Box<dyn StdError>> {
        use std::process::Command;

        let mut gpus = Vec::new();

        // Try to enumerate GPU devices from /sys/class/drm/card*
        let drm_entries: Vec<_> = std::fs::read_dir("/sys/class/drm")
            .unwrap_or_else(|_| std::fs::read_dir("/dev/null").unwrap()) // fallback to empty iterator
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                // Match card0, card1, etc. but not card0-HDMI-1, etc.
                name.starts_with("card") && name[4..].chars().all(|c| c.is_ascii_digit())
            })
            .collect();

        for entry in &drm_entries {
            let card_path = entry.path();
            let device_path = card_path.join("device");

            // Read PCI vendor ID
            let vendor_id_str = std::fs::read_to_string(device_path.join("vendor"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let vendor_id = u32::from_str_radix(
                vendor_id_str.trim_start_matches("0x"),
                16,
            ).unwrap_or(0);

            let vendor = match vendor_id {
                0x10de => "NVIDIA".to_string(),
                0x1002 => "AMD".to_string(),
                0x8086 => "Intel".to_string(),
                0x106b => "Apple".to_string(),
                0x1a03 => "ASPEED".to_string(),
                0 => "Unknown".to_string(),
                other => format!("Unknown (0x{:04x})", other),
            };

            // Try to get GPU name from lspci
            let name = {
                // First try device label from sysfs
                let label = std::fs::read_to_string(device_path.join("label"))
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());

                if let Some(label) = label {
                    label
                } else {
                    // Fall back to lspci to get a human-readable name
                    let pci_slot = device_path.file_name()
                        .and_then(|_| {
                            // Read the uevent to get PCI_SLOT_NAME
                            std::fs::read_to_string(device_path.join("uevent")).ok()
                        })
                        .and_then(|uevent| {
                            for line in uevent.lines() {
                                if let Some(slot) = line.strip_prefix("PCI_SLOT_NAME=") {
                                    return Some(slot.to_string());
                                }
                            }
                            None
                        });

                    if let Some(slot) = pci_slot {
                        // Run lspci -s <slot> -mm to get device name
                        Command::new("lspci")
                            .args(["-s", &slot])
                            .output()
                            .ok()
                            .and_then(|output| {
                                if output.status.success() {
                                    let line = String::from_utf8_lossy(&output.stdout).to_string();
                                    // lspci output: "00:02.0 VGA compatible controller: Intel Corporation ..."
                                    line.split_once(':')
                                        .and_then(|(_, rest)| rest.split_once(':'))
                                        .map(|(_, name)| name.trim().to_string())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| format!("{} GPU", vendor))
                    } else {
                        format!("{} GPU", vendor)
                    }
                }
            };

            // VRAM: try multiple sysfs paths
            let vram_mb = {
                // AMD: mem_info_vram_total (bytes)
                let amd_vram = std::fs::read_to_string(device_path.join("mem_info_vram_total"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .map(|bytes| bytes / (1024 * 1024));

                if let Some(mb) = amd_vram {
                    mb
                } else {
                    // Intel: try resource file or leave 0
                    // NVIDIA: try /proc/driver/nvidia/gpus/*/information
                    let card_name = entry.file_name().to_string_lossy().to_string();
                    let card_num = card_name.trim_start_matches("card");
                    nvidia_vram_from_proc(card_num).unwrap_or(0)
                }
            };

            // Driver version
            let driver_version = if vendor_id == 0x10de {
                // NVIDIA: read from /sys/module/nvidia/version
                std::fs::read_to_string("/sys/module/nvidia/version")
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            } else if vendor_id == 0x1002 {
                // AMD: read from /sys/module/amdgpu/version or kernel version
                std::fs::read_to_string("/sys/module/amdgpu/version")
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            } else {
                // Intel and others: use kernel version as driver version
                std::fs::read_to_string("/proc/version")
                    .ok()
                    .and_then(|s| {
                        s.split_whitespace()
                            .nth(2)
                            .map(|v| v.to_string())
                    })
                    .unwrap_or_default()
            };

            gpus.push(GpuInfo {
                name,
                vendor,
                vram_mb,
                driver_version,
            });
        }

        // If no GPUs found via sysfs, try lspci as fallback
        if gpus.is_empty() {
            if let Ok(output) = Command::new("lspci").output() {
                if output.status.success() {
                    let lspci_output = String::from_utf8_lossy(&output.stdout);
                    for line in lspci_output.lines() {
                        if line.contains("VGA") || line.contains("3D controller") || line.contains("Display controller") {
                            // Parse: "00:02.0 VGA compatible controller: Intel Corporation Device Name"
                            let name = line.split_once(':')
                                .and_then(|(_, rest)| rest.split_once(':'))
                                .map(|(_, name)| name.trim().to_string())
                                .unwrap_or_else(|| line.to_string());

                            let vendor = if name.contains("NVIDIA") || name.contains("GeForce") {
                                "NVIDIA".to_string()
                            } else if name.contains("AMD") || name.contains("Radeon") || name.contains("ATI") {
                                "AMD".to_string()
                            } else if name.contains("Intel") {
                                "Intel".to_string()
                            } else {
                                "Unknown".to_string()
                            };

                            gpus.push(GpuInfo {
                                name,
                                vendor,
                                vram_mb: 0,
                                driver_version: String::new(),
                            });
                        }
                    }
                }
            }
        }

        Ok(gpus)
    }

    #[cfg(feature = "battery")]
    fn battery_info(&self) -> Result<Option<BatteryInfo>, Box<dyn StdError>> {
        // Look for battery entries in /sys/class/power_supply/
        let power_supply_dir = std::path::Path::new("/sys/class/power_supply");
        if !power_supply_dir.exists() {
            return Ok(None);
        }

        let entries = std::fs::read_dir(power_supply_dir)
            .map_err(|e| format!("Failed to read /sys/class/power_supply: {}", e))?;

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Check if this is a battery (type == "Battery")
            let type_path = path.join("type");
            let supply_type = match std::fs::read_to_string(&type_path) {
                Ok(t) => t.trim().to_string(),
                Err(_) => continue,
            };

            if supply_type != "Battery" {
                continue;
            }

            // charge_percent from "capacity" file (0-100)
            let charge_percent = std::fs::read_to_string(path.join("capacity"))
                .ok()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .unwrap_or(0.0);

            // Battery status from "status" file
            let status_str = std::fs::read_to_string(path.join("status"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let status = match status_str.as_str() {
                "Charging" => BatteryStatus::Charging,
                "Discharging" => BatteryStatus::Discharging,
                "Full" => BatteryStatus::Full,
                "Not charging" => BatteryStatus::NotCharging,
                _ => BatteryStatus::Unknown,
            };

            // Cycle count
            let cycle_count = std::fs::read_to_string(path.join("cycle_count"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());

            // Design capacity: energy_full_design is in uWh, convert to mWh
            // Some systems use charge_full_design (uAh) instead
            let design_capacity_mwh = std::fs::read_to_string(path.join("energy_full_design"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|uwh| uwh / 1000)
                .or_else(|| {
                    // Fallback: charge_full_design (uAh) * voltage_min_design (uV) / 1e9 => mWh
                    let charge_uah = std::fs::read_to_string(path.join("charge_full_design"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())?;
                    let voltage_uv = std::fs::read_to_string(path.join("voltage_min_design"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .unwrap_or(3_700_000); // default ~3.7V
                    Some(charge_uah * voltage_uv / 1_000_000_000)
                });

            // Full charge capacity: energy_full is in uWh, convert to mWh
            let full_charge_capacity_mwh = std::fs::read_to_string(path.join("energy_full"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|uwh| uwh / 1000)
                .or_else(|| {
                    let charge_uah = std::fs::read_to_string(path.join("charge_full"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())?;
                    let voltage_uv = std::fs::read_to_string(path.join("voltage_min_design"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .unwrap_or(3_700_000);
                    Some(charge_uah * voltage_uv / 1_000_000_000)
                });

            // Health percent: full_charge / design_capacity * 100
            let health_percent = match (full_charge_capacity_mwh, design_capacity_mwh) {
                (Some(full), Some(design)) if design > 0 => {
                    Some((full as f64 / design as f64) * 100.0)
                }
                _ => None,
            };

            // Time to empty: try time_to_empty_avg first, then compute from energy_now / power_now
            let time_to_empty_secs = std::fs::read_to_string(path.join("time_to_empty_avg"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|secs| secs)
                .or_else(|| {
                    if status != BatteryStatus::Discharging {
                        return None;
                    }
                    let energy_now = std::fs::read_to_string(path.join("energy_now"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())?;
                    let power_now = std::fs::read_to_string(path.join("power_now"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .filter(|&p| p > 0)?;
                    Some(energy_now * 3600 / power_now)
                });

            // Time to full: try time_to_full_avg, then compute from remaining capacity / power
            let time_to_full_secs = std::fs::read_to_string(path.join("time_to_full_avg"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .or_else(|| {
                    if status != BatteryStatus::Charging {
                        return None;
                    }
                    let energy_now = std::fs::read_to_string(path.join("energy_now"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())?;
                    let energy_full = std::fs::read_to_string(path.join("energy_full"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())?;
                    let power_now = std::fs::read_to_string(path.join("power_now"))
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .filter(|&p| p > 0)?;
                    let remaining = energy_full.saturating_sub(energy_now);
                    Some(remaining * 3600 / power_now)
                });

            // Return the first battery found
            return Ok(Some(BatteryInfo {
                charge_percent,
                status,
                health_percent,
                cycle_count,
                design_capacity_mwh,
                full_charge_capacity_mwh,
                time_to_empty_secs,
                time_to_full_secs,
            }));
        }

        // No battery found
        Ok(None)
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
        let mut thermals = Vec::new();

        // Read from /sys/class/thermal/thermal_zone*/
        let thermal_dir = std::path::Path::new("/sys/class/thermal");
        if let Ok(entries) = std::fs::read_dir(thermal_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("thermal_zone") {
                    continue;
                }

                let zone_path = entry.path();

                // Read temperature (in millidegrees Celsius)
                let temp_mc = match std::fs::read_to_string(zone_path.join("temp")) {
                    Ok(s) => match s.trim().parse::<i64>() {
                        Ok(v) => v,
                        Err(_) => continue,
                    },
                    Err(_) => continue,
                };
                let temperature_celsius = temp_mc as f64 / 1000.0;

                // Read label (type file)
                let label = std::fs::read_to_string(zone_path.join("type"))
                    .unwrap_or_else(|_| name.clone())
                    .trim()
                    .to_string();

                // Find critical temperature from trip points
                let critical_celsius = (0..20u32).find_map(|i| {
                    let trip_type_path = zone_path.join(format!("trip_point_{}_type", i));
                    let trip_temp_path = zone_path.join(format!("trip_point_{}_temp", i));

                    let trip_type = std::fs::read_to_string(&trip_type_path).ok()?;
                    if trip_type.trim() == "critical" {
                        let trip_temp = std::fs::read_to_string(&trip_temp_path).ok()?;
                        let mc = trip_temp.trim().parse::<i64>().ok()?;
                        Some(mc as f64 / 1000.0)
                    } else {
                        None
                    }
                });

                thermals.push(ThermalInfo {
                    label,
                    temperature_celsius,
                    critical_celsius,
                });
            }
        }

        // Also read from /sys/class/hwmon/hwmon*/
        let hwmon_dir = std::path::Path::new("/sys/class/hwmon");
        if let Ok(entries) = std::fs::read_dir(hwmon_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let hwmon_path = entry.path();

                // Enumerate temp*_input files
                if let Ok(files) = std::fs::read_dir(&hwmon_path) {
                    for file in files.filter_map(|f| f.ok()) {
                        let fname = file.file_name().to_string_lossy().to_string();
                        if !fname.starts_with("temp") || !fname.ends_with("_input") {
                            continue;
                        }

                        // Extract the temp index (e.g., "temp1_input" -> "1")
                        let idx = &fname[4..fname.len() - 6]; // strip "temp" and "_input"

                        // Read temperature in millidegrees
                        let temp_mc = match std::fs::read_to_string(file.path()) {
                            Ok(s) => match s.trim().parse::<i64>() {
                                Ok(v) => v,
                                Err(_) => continue,
                            },
                            Err(_) => continue,
                        };
                        let temperature_celsius = temp_mc as f64 / 1000.0;

                        // Read label: try temp*_label first, fall back to hwmon name
                        let label_path = hwmon_path.join(format!("temp{}_label", idx));
                        let hwmon_name_path = hwmon_path.join("name");
                        let label = std::fs::read_to_string(&label_path)
                            .or_else(|_| std::fs::read_to_string(&hwmon_name_path))
                            .unwrap_or_else(|_| {
                                entry.file_name().to_string_lossy().to_string()
                            })
                            .trim()
                            .to_string();

                        // Read critical temperature
                        let crit_path = hwmon_path.join(format!("temp{}_crit", idx));
                        let critical_celsius = std::fs::read_to_string(&crit_path)
                            .ok()
                            .and_then(|s| s.trim().parse::<i64>().ok())
                            .map(|mc| mc as f64 / 1000.0);

                        thermals.push(ThermalInfo {
                            label,
                            temperature_celsius,
                            critical_celsius,
                        });
                    }
                }
            }
        }

        Ok(thermals)
    }

    #[cfg(feature = "display")]
    fn display_info(&self) -> Result<Vec<DisplayInfo>, Box<dyn StdError>> {
        // Try xrandr first (works on X11)
        match parse_xrandr_displays() {
            Ok(displays) if !displays.is_empty() => return Ok(displays),
            _ => {}
        }

        // Fallback: read from /sys/class/drm for basic info
        let mut displays = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            let mut is_first = true;
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                // Match connector entries like card0-HDMI-A-1, card0-eDP-1, etc.
                if !name.contains('-') || !name.starts_with("card") {
                    continue;
                }

                let connector_path = entry.path();

                // Check if connected
                let status = std::fs::read_to_string(connector_path.join("status"))
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if status != "connected" {
                    continue;
                }

                // Read modes (first line is preferred mode)
                let modes_str = std::fs::read_to_string(connector_path.join("modes"))
                    .unwrap_or_default();
                let first_mode = modes_str.lines().next().unwrap_or("");

                let (width, height) = if let Some((w, h)) = first_mode.split_once('x') {
                    (w.parse::<u32>().unwrap_or(0), h.parse::<u32>().unwrap_or(0))
                } else {
                    continue;
                };

                if width == 0 || height == 0 {
                    continue;
                }

                let connector_name = name.split_once('-')
                    .map(|(_, rest)| rest.to_string())
                    .unwrap_or(name.clone());

                displays.push(DisplayInfo {
                    name: connector_name,
                    width,
                    height,
                    dpi: 96.0,
                    refresh_rate_hz: None,
                    is_primary: is_first,
                });
                is_first = false;
            }
        }

        Ok(displays)
    }
}
