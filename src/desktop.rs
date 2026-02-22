use serde::de::DeserializeOwned;
use std::sync::{Arc, Mutex};
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;
use crate::platform;

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<System<R>> {
    let provider = platform::create_provider();
    Ok(System {
        _app_handle: app.clone(),
        provider: Arc::new(Mutex::new(provider)),
    })
}

pub struct System<R: Runtime> {
    _app_handle: AppHandle<R>,
    provider: Arc<Mutex<Box<dyn platform::SystemInfoProvider + Send>>>,
}

impl<R: Runtime> System<R> {
    #[cfg(feature = "os")]
    pub fn os_info(&self) -> crate::Result<OsInfo> {
        let provider = self.provider.lock().unwrap();
        provider
            .os_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "cpu")]
    pub fn cpu_info(&self) -> crate::Result<CpuInfo> {
        let provider = self.provider.lock().unwrap();
        provider
            .cpu_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "cpu")]
    pub fn cpu_usage(&self) -> crate::Result<Vec<f64>> {
        let provider = self.provider.lock().unwrap();
        provider
            .cpu_usage()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "memory")]
    pub fn memory_info(&self) -> crate::Result<MemoryInfo> {
        let provider = self.provider.lock().unwrap();
        provider
            .memory_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "disk")]
    pub fn disk_info(&self) -> crate::Result<Vec<DiskInfo>> {
        let provider = self.provider.lock().unwrap();
        provider
            .disk_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "gpu")]
    pub fn gpu_info(&self) -> crate::Result<Vec<GpuInfo>> {
        let provider = self.provider.lock().unwrap();
        provider
            .gpu_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "battery")]
    pub fn battery_info(&self) -> crate::Result<Option<BatteryInfo>> {
        let provider = self.provider.lock().unwrap();
        provider
            .battery_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "network")]
    pub fn network_info(&self) -> crate::Result<Vec<NetworkInfo>> {
        let provider = self.provider.lock().unwrap();
        provider
            .network_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "thermal")]
    pub fn thermal_info(&self) -> crate::Result<Vec<ThermalInfo>> {
        let provider = self.provider.lock().unwrap();
        provider
            .thermal_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    #[cfg(feature = "display")]
    pub fn display_info(&self) -> crate::Result<Vec<DisplayInfo>> {
        let provider = self.provider.lock().unwrap();
        provider
            .display_info()
            .map_err(|e| crate::Error::Platform(e.to_string()))
    }

    pub fn all_info(&self) -> crate::Result<SystemInfo> {
        Ok(SystemInfo {
            #[cfg(feature = "os")]
            os: self.os_info().ok(),
            #[cfg(feature = "cpu")]
            cpu: self.cpu_info().ok(),
            #[cfg(feature = "memory")]
            memory: self.memory_info().ok(),
            #[cfg(feature = "disk")]
            disks: self.disk_info().ok(),
            #[cfg(feature = "gpu")]
            gpus: self.gpu_info().ok(),
            #[cfg(feature = "battery")]
            battery: self.battery_info().ok().flatten(),
            #[cfg(feature = "network")]
            networks: self.network_info().ok(),
            #[cfg(feature = "thermal")]
            thermals: self.thermal_info().ok(),
            #[cfg(feature = "display")]
            displays: self.display_info().ok(),
        })
    }
}
