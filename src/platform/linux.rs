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
