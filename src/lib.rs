use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
mod desktop;

mod commands;
mod error;
mod models;
pub mod platform;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::System;

pub trait SystemExt<R: Runtime> {
    fn system(&self) -> &System<R>;
}

impl<R: Runtime, T: Manager<R>> crate::SystemExt<R> for T {
    fn system(&self) -> &System<R> {
        self.state::<System<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("system")
        .invoke_handler(tauri::generate_handler![
            #[cfg(feature = "os")]
            commands::get_os_info,
            #[cfg(feature = "cpu")]
            commands::get_cpu_info,
            #[cfg(feature = "cpu")]
            commands::get_cpu_usage,
            #[cfg(feature = "memory")]
            commands::get_memory_info,
            #[cfg(feature = "disk")]
            commands::get_disk_info,
            #[cfg(feature = "gpu")]
            commands::get_gpu_info,
            #[cfg(feature = "battery")]
            commands::get_battery_info,
            #[cfg(feature = "network")]
            commands::get_network_info,
            #[cfg(feature = "thermal")]
            commands::get_thermal_info,
            #[cfg(feature = "display")]
            commands::get_display_info,
            commands::get_all_info,
        ])
        .setup(|app, api| {
            #[cfg(desktop)]
            let system = desktop::init(app, api)?;
            app.manage(system);
            Ok(())
        })
        .build()
}
