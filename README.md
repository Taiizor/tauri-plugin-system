# Tauri Plugin System

[![Crates.io](https://img.shields.io/crates/v/tauri-plugin-system.svg)](https://crates.io/crates/tauri-plugin-system)
[![npm](https://img.shields.io/npm/v/tauri-plugin-system-api.svg)](https://www.npmjs.com/package/tauri-plugin-system-api)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://github.com/Taiizor/tauri-plugin-system/blob/develop/LICENSE)

A comprehensive cross-platform system information plugin for Tauri v2. Provides detailed hardware and OS data through native platform APIs — no third-party system info crates.

## Features

- **CPU**: Model, vendor, cores, threads, architecture, frequency, per-core usage
- **Memory**: Total, used, free, available, swap usage
- **Disk**: Name, mount point, filesystem, SSD/HDD detection, capacity
- **GPU**: Name, vendor, VRAM, driver version (via DXGI/system_profiler/sysfs)
- **Battery**: Charge level, status, health, cycle count, capacity
- **Network**: Interface name, MAC, IPv4/IPv6, RX/TX bytes, up/down status
- **Thermal**: Sensor label, temperature, critical threshold
- **Display**: Name, resolution, DPI, refresh rate, primary flag
- **OS**: Name, version, hostname, architecture, uptime, username
- **Feature Flags**: Enable only the modules you need via Cargo features
- **Zero Dependencies**: Uses native OS APIs only (Win32/DXGI/WDK, sysctl/IOKit, procfs/sysfs)
- **Type Safety**: Full TypeScript typings for all data structures

## Platform Support

| Platform | Status | Backend |
|----------|--------|---------|
| Windows  | Full   | Win32, DXGI, WDK, Registry |
| macOS    | Full   | sysctl, mach2, system_profiler, IOKit |
| Linux    | Full   | procfs, sysfs, xrandr, lspci |

## Installation

### Using Tauri CLI (Recommended)

```bash
# Using npm
npm run tauri add system

# Using pnpm
pnpm tauri add system

# Using yarn
yarn tauri add system

# Using bun
bun tauri add system
```

### Manual Installation

#### Rust Dependencies

```bash
cargo add tauri-plugin-system
```

Or add to your `Cargo.toml`:

```toml
[dependencies]
tauri-plugin-system = "0.1.0"
```

To enable only specific modules:

```toml
[dependencies]
tauri-plugin-system = { version = "0.1.0", default-features = false, features = ["cpu", "memory", "gpu"] }
```

#### JavaScript/TypeScript API

```bash
pnpm install tauri-plugin-system-api
# or
npm install tauri-plugin-system-api
```

## Setup

Register the plugin in your Tauri application:

```rust
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_system::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

## Permissions

Add the plugin permission to your capability configuration:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "system-access",
  "description": "Access system information",
  "windows": ["main"],
  "permissions": [
    "system:default"
  ]
}
```

### Default Permission

The `system:default` permission grants access to all system info commands:

- `system:allow-get-os-info`
- `system:allow-get-cpu-info`
- `system:allow-get-cpu-usage`
- `system:allow-get-memory-info`
- `system:allow-get-disk-info`
- `system:allow-get-gpu-info`
- `system:allow-get-battery-info`
- `system:allow-get-network-info`
- `system:allow-get-thermal-info`
- `system:allow-get-display-info`
- `system:allow-get-all-info`

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `os`      | Yes | Operating system information |
| `cpu`     | Yes | Processor details and per-core usage |
| `memory`  | Yes | RAM and swap usage |
| `disk`    | Yes | Disk capacity and SSD/HDD detection |
| `gpu`     | No  | Graphics card details via DXGI/system_profiler/sysfs |
| `battery` | No  | Battery status and health |
| `network` | No  | Network interface details and traffic |
| `thermal` | No  | Temperature sensor readings |
| `display` | No  | Display resolution, DPI, and refresh rate |
| `all`     | No  | Enables all modules |

## Usage

### JavaScript/TypeScript

```typescript
import {
  osInfo, cpuInfo, cpuUsage, memoryInfo,
  diskInfo, gpuInfo, batteryInfo, networkInfo,
  thermalInfo, displayInfo, allInfo
} from 'tauri-plugin-system-api';

// Get OS information
const os = await osInfo();
console.log(`${os.name} ${os.version} (${os.arch})`);

// Get CPU details
const cpu = await cpuInfo();
console.log(`${cpu.model} - ${cpu.cores} cores / ${cpu.threads} threads`);

// Get per-core CPU usage
const usage = await cpuUsage();
usage.forEach((pct, i) => console.log(`Core ${i}: ${pct.toFixed(1)}%`));

// Get memory info
const mem = await memoryInfo();
console.log(`RAM: ${(mem.usedBytes / 1e9).toFixed(1)} / ${(mem.totalBytes / 1e9).toFixed(1)} GB`);

// Get all disks
const disks = await diskInfo();
disks.forEach(d => console.log(`${d.mountPoint} (${d.kind}): ${d.fsType}`));

// Get GPU info
const gpus = await gpuInfo();
gpus.forEach(g => console.log(`${g.name} - ${g.vramMb} MB VRAM`));

// Get battery status (null on desktops without battery)
const bat = await batteryInfo();
if (bat) console.log(`Battery: ${bat.chargePercent}% (${bat.status})`);

// Get all system info at once
const sys = await allInfo();
console.log(JSON.stringify(sys, null, 2));
```

### Rust

```rust
use tauri::Manager;
use tauri_plugin_system::SystemExt;

#[tauri::command]
async fn get_system_summary(app: tauri::AppHandle) -> Result<String, String> {
    let system = app.system();

    let os = system.os_info().map_err(|e| e.to_string())?;
    let cpu = system.cpu_info().map_err(|e| e.to_string())?;
    let mem = system.memory_info().map_err(|e| e.to_string())?;

    Ok(format!("{} {} - {} ({} cores) - {:.1} GB RAM",
        os.name, os.version,
        cpu.model, cpu.cores,
        mem.total_bytes as f64 / 1e9
    ))
}
```

## License

This project is released under the [MIT License](https://github.com/Taiizor/tauri-plugin-system/blob/develop/LICENSE).