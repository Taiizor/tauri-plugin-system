use serde::{Deserialize, Serialize};

// === OS Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub full_version: String,
    pub hostname: String,
    pub arch: String,
    pub uptime_secs: u64,
    pub username: String,
}

// === CPU Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub model: String,
    pub vendor: String,
    pub cores: u32,
    pub threads: u32,
    pub arch: String,
    pub frequency_mhz: u64,
}

// === Memory Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

// === Disk Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub fs_type: String,
    pub kind: DiskKind,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub is_removable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskKind {
    Ssd,
    Hdd,
    Unknown,
}

// === GPU Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub vram_mb: u64,
    pub driver_version: String,
}

// === Battery Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    pub charge_percent: f64,
    pub status: BatteryStatus,
    pub health_percent: Option<f64>,
    pub cycle_count: Option<u32>,
    pub design_capacity_mwh: Option<u64>,
    pub full_charge_capacity_mwh: Option<u64>,
    pub time_to_empty_secs: Option<u64>,
    pub time_to_full_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

// === Network Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub name: String,
    pub mac_address: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub is_up: bool,
}

// === Thermal Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThermalInfo {
    pub label: String,
    pub temperature_celsius: f64,
    pub critical_celsius: Option<f64>,
}

// === Display Module ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub dpi: f64,
    pub refresh_rate_hz: Option<f64>,
    pub is_primary: bool,
}

// === Aggregated ===
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    #[cfg(feature = "os")]
    pub os: Option<OsInfo>,
    #[cfg(feature = "cpu")]
    pub cpu: Option<CpuInfo>,
    #[cfg(feature = "memory")]
    pub memory: Option<MemoryInfo>,
    #[cfg(feature = "disk")]
    pub disks: Option<Vec<DiskInfo>>,
    #[cfg(feature = "gpu")]
    pub gpus: Option<Vec<GpuInfo>>,
    #[cfg(feature = "battery")]
    pub battery: Option<BatteryInfo>,
    #[cfg(feature = "network")]
    pub networks: Option<Vec<NetworkInfo>>,
    #[cfg(feature = "thermal")]
    pub thermals: Option<Vec<ThermalInfo>>,
    #[cfg(feature = "display")]
    pub displays: Option<Vec<DisplayInfo>>,
}
