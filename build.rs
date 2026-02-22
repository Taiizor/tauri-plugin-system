const COMMANDS: &[&str] = &[
    "get_cpu_info",
    "get_cpu_usage",
    "get_memory_info",
    "get_disk_info",
    "get_gpu_info",
    "get_battery_info",
    "get_network_info",
    "get_thermal_info",
    "get_display_info",
    "get_os_info",
    "get_all_info",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
