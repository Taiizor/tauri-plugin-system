use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::models::*;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.taiizor.tauri.plugin.system";

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_system);

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<System<R>> {
    #[cfg(target_os = "android")]
    let handle = api
        .register_android_plugin(PLUGIN_IDENTIFIER, "SystemPlugin")
        .map_err(|e| crate::Error::PluginInvoke(e.to_string()))?;

    #[cfg(target_os = "ios")]
    let handle = api
        .register_ios_plugin(init_plugin_system)
        .map_err(|e| crate::Error::PluginInvoke(e.to_string()))?;

    Ok(System(handle))
}

pub struct System<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> System<R> {
    #[cfg(feature = "os")]
    pub fn os_info(&self) -> crate::Result<OsInfo> {
        self.0
            .run_mobile_plugin("get_os_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "cpu")]
    pub fn cpu_info(&self) -> crate::Result<CpuInfo> {
        self.0
            .run_mobile_plugin("get_cpu_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "cpu")]
    pub fn cpu_usage(&self) -> crate::Result<Vec<f64>> {
        self.0
            .run_mobile_plugin("get_cpu_usage", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "memory")]
    pub fn memory_info(&self) -> crate::Result<MemoryInfo> {
        self.0
            .run_mobile_plugin("get_memory_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "disk")]
    pub fn disk_info(&self) -> crate::Result<Vec<DiskInfo>> {
        self.0
            .run_mobile_plugin("get_disk_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "gpu")]
    pub fn gpu_info(&self) -> crate::Result<Vec<GpuInfo>> {
        self.0
            .run_mobile_plugin("get_gpu_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "battery")]
    pub fn battery_info(&self) -> crate::Result<Option<BatteryInfo>> {
        self.0
            .run_mobile_plugin("get_battery_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "network")]
    pub fn network_info(&self) -> crate::Result<Vec<NetworkInfo>> {
        self.0
            .run_mobile_plugin("get_network_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "thermal")]
    pub fn thermal_info(&self) -> crate::Result<Vec<ThermalInfo>> {
        self.0
            .run_mobile_plugin("get_thermal_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    #[cfg(feature = "display")]
    pub fn display_info(&self) -> crate::Result<Vec<DisplayInfo>> {
        self.0
            .run_mobile_plugin("get_display_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }

    pub fn all_info(&self) -> crate::Result<SystemInfo> {
        self.0
            .run_mobile_plugin("get_all_info", ())
            .map_err(|e| crate::Error::PluginInvoke(e.to_string()))
    }
}
