import { invoke } from '@tauri-apps/api/core';

// === Types ===

export interface OsInfo {
  name: string;
  version: string;
  fullVersion: string;
  hostname: string;
  arch: string;
  uptimeSecs: number;
  username: string;
}

export interface CpuInfo {
  model: string;
  vendor: string;
  cores: number;
  threads: number;
  arch: string;
  frequencyMhz: number;
}

export interface MemoryInfo {
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
  availableBytes: number;
  swapTotalBytes: number;
  swapUsedBytes: number;
}

export interface DiskInfo {
  name: string;
  mountPoint: string;
  fsType: string;
  kind: 'ssd' | 'hdd' | 'unknown';
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
  isRemovable: boolean;
}

export interface GpuInfo {
  name: string;
  vendor: string;
  vramMb: number;
  driverVersion: string;
}

export interface BatteryInfo {
  chargePercent: number;
  status: 'charging' | 'discharging' | 'full' | 'notCharging' | 'unknown';
  healthPercent: number | null;
  cycleCount: number | null;
  designCapacityMwh: number | null;
  fullChargeCapacityMwh: number | null;
  timeToEmptySecs: number | null;
  timeToFullSecs: number | null;
}

export interface NetworkInfo {
  name: string;
  macAddress: string;
  ipv4: string[];
  ipv6: string[];
  rxBytes: number;
  txBytes: number;
  isUp: boolean;
}

export interface ThermalInfo {
  label: string;
  temperatureCelsius: number;
  criticalCelsius: number | null;
}

export interface DisplayInfo {
  name: string;
  width: number;
  height: number;
  dpi: number;
  refreshRateHz: number | null;
  isPrimary: boolean;
}

export interface SystemInfo {
  os?: OsInfo;
  cpu?: CpuInfo;
  memory?: MemoryInfo;
  disks?: DiskInfo[];
  gpus?: GpuInfo[];
  battery?: BatteryInfo;
  networks?: NetworkInfo[];
  thermals?: ThermalInfo[];
  displays?: DisplayInfo[];
}

// === API Functions ===

export async function osInfo(): Promise<OsInfo> {
  return invoke('plugin:system|get_os_info');
}

export async function cpuInfo(): Promise<CpuInfo> {
  return invoke('plugin:system|get_cpu_info');
}

export async function cpuUsage(): Promise<number[]> {
  return invoke('plugin:system|get_cpu_usage');
}

export async function memoryInfo(): Promise<MemoryInfo> {
  return invoke('plugin:system|get_memory_info');
}

export async function diskInfo(): Promise<DiskInfo[]> {
  return invoke('plugin:system|get_disk_info');
}

export async function gpuInfo(): Promise<GpuInfo[]> {
  return invoke('plugin:system|get_gpu_info');
}

export async function batteryInfo(): Promise<BatteryInfo | null> {
  return invoke('plugin:system|get_battery_info');
}

export async function networkInfo(): Promise<NetworkInfo[]> {
  return invoke('plugin:system|get_network_info');
}

export async function thermalInfo(): Promise<ThermalInfo[]> {
  return invoke('plugin:system|get_thermal_info');
}

export async function displayInfo(): Promise<DisplayInfo[]> {
  return invoke('plugin:system|get_display_info');
}

export async function allInfo(): Promise<SystemInfo> {
  return invoke('plugin:system|get_all_info');
}
