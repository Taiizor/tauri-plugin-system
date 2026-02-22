use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::Result;
use crate::SystemExt;

#[cfg(feature = "os")]
#[command]
pub(crate) async fn get_os_info<R: Runtime>(app: AppHandle<R>) -> Result<OsInfo> {
    app.system().os_info()
}

#[cfg(feature = "cpu")]
#[command]
pub(crate) async fn get_cpu_info<R: Runtime>(app: AppHandle<R>) -> Result<CpuInfo> {
    app.system().cpu_info()
}

#[cfg(feature = "cpu")]
#[command]
pub(crate) async fn get_cpu_usage<R: Runtime>(app: AppHandle<R>) -> Result<Vec<f64>> {
    app.system().cpu_usage()
}

#[cfg(feature = "memory")]
#[command]
pub(crate) async fn get_memory_info<R: Runtime>(app: AppHandle<R>) -> Result<MemoryInfo> {
    app.system().memory_info()
}

#[cfg(feature = "disk")]
#[command]
pub(crate) async fn get_disk_info<R: Runtime>(app: AppHandle<R>) -> Result<Vec<DiskInfo>> {
    app.system().disk_info()
}

#[cfg(feature = "gpu")]
#[command]
pub(crate) async fn get_gpu_info<R: Runtime>(app: AppHandle<R>) -> Result<Vec<GpuInfo>> {
    app.system().gpu_info()
}

#[cfg(feature = "battery")]
#[command]
pub(crate) async fn get_battery_info<R: Runtime>(app: AppHandle<R>) -> Result<Option<BatteryInfo>> {
    app.system().battery_info()
}

#[cfg(feature = "network")]
#[command]
pub(crate) async fn get_network_info<R: Runtime>(app: AppHandle<R>) -> Result<Vec<NetworkInfo>> {
    app.system().network_info()
}

#[cfg(feature = "thermal")]
#[command]
pub(crate) async fn get_thermal_info<R: Runtime>(app: AppHandle<R>) -> Result<Vec<ThermalInfo>> {
    app.system().thermal_info()
}

#[cfg(feature = "display")]
#[command]
pub(crate) async fn get_display_info<R: Runtime>(app: AppHandle<R>) -> Result<Vec<DisplayInfo>> {
    app.system().display_info()
}

#[command]
pub(crate) async fn get_all_info<R: Runtime>(app: AppHandle<R>) -> Result<SystemInfo> {
    app.system().all_info()
}
