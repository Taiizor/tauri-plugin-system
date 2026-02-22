import UIKit
import Metal
import Tauri
import WebKit

class SystemPlugin: Plugin {

    // MARK: - OS

    @objc public func get_os_info(_ invoke: Invoke) throws {
        let device = UIDevice.current
        let processInfo = ProcessInfo.processInfo

        var systemInfo = utsname()
        uname(&systemInfo)
        let arch = withUnsafePointer(to: &systemInfo.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: 1) {
                String(cString: $0)
            }
        }

        invoke.resolve([
            "name": "iOS",
            "version": device.systemVersion,
            "fullVersion": "\(device.systemName) \(device.systemVersion)",
            "hostname": processInfo.hostName,
            "arch": arch,
            "uptimeSecs": Int(processInfo.systemUptime),
            "username": device.name
        ])
    }

    // MARK: - CPU

    @objc public func get_cpu_info(_ invoke: Invoke) throws {
        let processInfo = ProcessInfo.processInfo
        let cores = processInfo.processorCount
        let activeCores = processInfo.activeProcessorCount

        var systemInfo = utsname()
        uname(&systemInfo)
        let machine = withUnsafePointer(to: &systemInfo.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: 1) {
                String(cString: $0)
            }
        }

        let model = sysctlString("machdep.cpu.brand_string") ?? machine
        let vendor = machine.hasPrefix("arm") || machine.hasPrefix("Apple") ? "Apple" : "Unknown"

        invoke.resolve([
            "model": model,
            "vendor": vendor,
            "cores": cores,
            "threads": activeCores,
            "arch": machine,
            "frequencyMhz": 0
        ])
    }

    @objc public func get_cpu_usage(_ invoke: Invoke) throws {
        let cores = ProcessInfo.processInfo.processorCount
        var usageArray: [Double] = []

        var processorInfo: processor_info_array_t?
        var processorMsgCount: mach_msg_type_number_t = 0
        var processorCount: natural_t = 0

        let result = host_processor_info(
            mach_host_self(),
            PROCESSOR_CPU_LOAD_INFO,
            &processorCount,
            &processorInfo,
            &processorMsgCount
        )

        if result == KERN_SUCCESS, let info = processorInfo {
            for i in 0..<Int(processorCount) {
                let offset = Int(CPU_STATE_MAX) * i
                let user = Double(info[offset + Int(CPU_STATE_USER)])
                let system = Double(info[offset + Int(CPU_STATE_SYSTEM)])
                let idle = Double(info[offset + Int(CPU_STATE_IDLE)])
                let nice = Double(info[offset + Int(CPU_STATE_NICE)])
                let total = user + system + idle + nice
                let usage = total > 0 ? ((user + system + nice) / total) * 100.0 : 0.0
                usageArray.append(min(max(usage, 0.0), 100.0))
            }

            let size = Int(processorMsgCount) * MemoryLayout<integer_t>.size
            vm_deallocate(mach_task_self_, vm_address_t(bitPattern: info), vm_size_t(size))
        } else {
            for _ in 0..<cores {
                usageArray.append(0.0)
            }
        }

        invoke.resolve(["value": usageArray])
    }

    // MARK: - Memory

    @objc public func get_memory_info(_ invoke: Invoke) throws {
        let processInfo = ProcessInfo.processInfo
        let totalMemory = processInfo.physicalMemory

        var available: UInt64 = 0
        if #available(iOS 13.0, *) {
            available = UInt64(os_proc_available_memory())
        }

        let used = totalMemory - available

        invoke.resolve([
            "totalBytes": totalMemory,
            "usedBytes": used,
            "freeBytes": available,
            "availableBytes": available,
            "swapTotalBytes": 0,
            "swapUsedBytes": 0
        ])
    }

    // MARK: - Disk

    @objc public func get_disk_info(_ invoke: Invoke) throws {
        var disks: [[String: Any]] = []

        if let attrs = try? FileManager.default.attributesOfFileSystem(
            forPath: NSHomeDirectory()
        ) {
            let totalBytes = (attrs[.systemSize] as? UInt64) ?? 0
            let freeBytes = (attrs[.systemFreeSize] as? UInt64) ?? 0

            var total = totalBytes
            var free = freeBytes
            if let url = URL(string: NSHomeDirectory()) {
                let resourceValues = try? url.resourceValues(forKeys: [
                    .volumeTotalCapacityKey,
                    .volumeAvailableCapacityForImportantUsageKey
                ])
                if let t = resourceValues?.volumeTotalCapacity {
                    total = UInt64(t)
                }
                if let f = resourceValues?.volumeAvailableCapacityForImportantUsage {
                    free = UInt64(f)
                }
            }

            disks.append([
                "name": "iPhone Storage",
                "mountPoint": "/",
                "fsType": "apfs",
                "kind": "ssd",
                "totalBytes": total,
                "usedBytes": total - free,
                "freeBytes": free,
                "isRemovable": false
            ])
        }

        invoke.resolve(["value": disks])
    }

    // MARK: - GPU

    @objc public func get_gpu_info(_ invoke: Invoke) throws {
        var gpus: [[String: Any]] = []

        if let device = MTLCreateSystemDefaultDevice() {
            gpus.append([
                "name": device.name,
                "vendor": "Apple",
                "vramMb": 0,
                "driverVersion": ""
            ])
        }

        invoke.resolve(["value": gpus])
    }

    // MARK: - Battery

    @objc public func get_battery_info(_ invoke: Invoke) throws {
        let device = UIDevice.current
        let wasMonitoring = device.isBatteryMonitoringEnabled
        device.isBatteryMonitoringEnabled = true

        let level = device.batteryLevel
        if level < 0 {
            device.isBatteryMonitoringEnabled = wasMonitoring
            invoke.resolve(["value": NSNull()])
            return
        }

        let chargePercent = Double(level) * 100.0

        let status: String
        switch device.batteryState {
        case .charging:
            status = "charging"
        case .full:
            status = "full"
        case .unplugged:
            status = "discharging"
        default:
            status = "unknown"
        }

        device.isBatteryMonitoringEnabled = wasMonitoring

        invoke.resolve([
            "chargePercent": chargePercent,
            "status": status,
            "healthPercent": NSNull(),
            "cycleCount": NSNull(),
            "designCapacityMwh": NSNull(),
            "fullChargeCapacityMwh": NSNull(),
            "timeToEmptySecs": NSNull(),
            "timeToFullSecs": NSNull()
        ])
    }

    // MARK: - Network

    @objc public func get_network_info(_ invoke: Invoke) throws {
        var networks: [[String: Any]] = []

        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&ifaddr) == 0, let firstAddr = ifaddr else {
            invoke.resolve(["value": networks])
            return
        }

        defer { freeifaddrs(ifaddr) }

        var interfaceMap: [String: [String: Any]] = [:]

        var ptr = firstAddr
        while true {
            let name = String(cString: ptr.pointee.ifa_name)
            let flags = Int32(ptr.pointee.ifa_flags)
            let isUp = (flags & IFF_UP) != 0 && (flags & IFF_LOOPBACK) == 0

            if (flags & IFF_LOOPBACK) == 0 {
                if interfaceMap[name] == nil {
                    interfaceMap[name] = [
                        "name": name,
                        "macAddress": "00:00:00:00:00:00",
                        "ipv4": [String](),
                        "ipv6": [String](),
                        "rxBytes": 0,
                        "txBytes": 0,
                        "isUp": isUp
                    ]
                }

                let family = ptr.pointee.ifa_addr.pointee.sa_family
                if family == UInt8(AF_INET) {
                    var addr = ptr.pointee.ifa_addr.withMemoryRebound(to: sockaddr_in.self, capacity: 1) {
                        $0.pointee.sin_addr
                    }
                    var buffer = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
                    inet_ntop(AF_INET, &addr, &buffer, socklen_t(INET_ADDRSTRLEN))
                    let ipv4 = String(cString: buffer)
                    var ipv4List = interfaceMap[name]!["ipv4"] as! [String]
                    ipv4List.append(ipv4)
                    interfaceMap[name]!["ipv4"] = ipv4List
                } else if family == UInt8(AF_INET6) {
                    var addr = ptr.pointee.ifa_addr.withMemoryRebound(to: sockaddr_in6.self, capacity: 1) {
                        $0.pointee.sin6_addr
                    }
                    var buffer = [CChar](repeating: 0, count: Int(INET6_ADDRSTRLEN))
                    inet_ntop(AF_INET6, &addr, &buffer, socklen_t(INET6_ADDRSTRLEN))
                    let ipv6 = String(cString: buffer)
                    var ipv6List = interfaceMap[name]!["ipv6"] as! [String]
                    ipv6List.append(ipv6)
                    interfaceMap[name]!["ipv6"] = ipv6List
                } else if family == UInt8(AF_LINK) {
                    let sdl = ptr.pointee.ifa_addr.withMemoryRebound(to: sockaddr_dl.self, capacity: 1) {
                        $0.pointee
                    }
                    let addrLen = Int(sdl.sdl_alen)
                    if addrLen == 6 {
                        let data = withUnsafePointer(to: sdl.sdl_data) {
                            $0.withMemoryRebound(to: UInt8.self, capacity: Int(sdl.sdl_nlen) + addrLen) {
                                Array(UnsafeBufferPointer(start: $0 + Int(sdl.sdl_nlen), count: addrLen))
                            }
                        }
                        let mac = data.map { String(format: "%02x", $0) }.joined(separator: ":")
                        interfaceMap[name]!["macAddress"] = mac
                    }
                }
            }

            guard let next = ptr.pointee.ifa_next else { break }
            ptr = next
        }

        networks = Array(interfaceMap.values)
        invoke.resolve(["value": networks])
    }

    // MARK: - Thermal

    @objc public func get_thermal_info(_ invoke: Invoke) throws {
        var thermals: [[String: Any]] = []

        let state = ProcessInfo.processInfo.thermalState
        let temp: Double
        let label: String
        switch state {
        case .nominal:
            temp = 25.0
            label = "Device Thermal (Nominal)"
        case .fair:
            temp = 35.0
            label = "Device Thermal (Fair)"
        case .serious:
            temp = 45.0
            label = "Device Thermal (Serious)"
        case .critical:
            temp = 55.0
            label = "Device Thermal (Critical)"
        @unknown default:
            temp = 25.0
            label = "Device Thermal (Unknown)"
        }

        thermals.append([
            "label": label,
            "temperatureCelsius": temp,
            "criticalCelsius": 80.0
        ])

        invoke.resolve(["value": thermals])
    }

    // MARK: - Display

    @objc public func get_display_info(_ invoke: Invoke) throws {
        var displays: [[String: Any]] = []

        let screen = UIScreen.main
        let bounds = screen.nativeBounds
        let scale = screen.nativeScale

        displays.append([
            "name": "Built-in Display",
            "width": Int(bounds.width),
            "height": Int(bounds.height),
            "dpi": Double(scale) * 163.0,
            "refreshRateHz": Double(screen.maximumFramesPerSecond),
            "isPrimary": true
        ])

        invoke.resolve(["value": displays])
    }

    // MARK: - All Info

    @objc public func get_all_info(_ invoke: Invoke) throws {
        var result: [String: Any] = [:]

        // OS
        let device = UIDevice.current
        let processInfo = ProcessInfo.processInfo
        var systemInfo = utsname()
        uname(&systemInfo)
        let arch = withUnsafePointer(to: &systemInfo.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: 1) {
                String(cString: $0)
            }
        }
        result["os"] = [
            "name": "iOS",
            "version": device.systemVersion,
            "fullVersion": "\(device.systemName) \(device.systemVersion)",
            "hostname": processInfo.hostName,
            "arch": arch,
            "uptimeSecs": Int(processInfo.systemUptime),
            "username": device.name
        ]

        // CPU
        let cores = processInfo.processorCount
        let machine = arch
        let model = sysctlString("machdep.cpu.brand_string") ?? machine
        result["cpu"] = [
            "model": model,
            "vendor": "Apple",
            "cores": cores,
            "threads": processInfo.activeProcessorCount,
            "arch": machine,
            "frequencyMhz": 0
        ]

        // Memory
        let totalMemory = processInfo.physicalMemory
        var available: UInt64 = 0
        if #available(iOS 13.0, *) {
            available = UInt64(os_proc_available_memory())
        }
        result["memory"] = [
            "totalBytes": totalMemory,
            "usedBytes": totalMemory - available,
            "freeBytes": available,
            "availableBytes": available,
            "swapTotalBytes": 0,
            "swapUsedBytes": 0
        ]

        // Disks
        if let attrs = try? FileManager.default.attributesOfFileSystem(forPath: NSHomeDirectory()) {
            let total = (attrs[.systemSize] as? UInt64) ?? 0
            let free = (attrs[.systemFreeSize] as? UInt64) ?? 0
            result["disks"] = [[
                "name": "iPhone Storage",
                "mountPoint": "/",
                "fsType": "apfs",
                "kind": "ssd",
                "totalBytes": total,
                "usedBytes": total - free,
                "freeBytes": free,
                "isRemovable": false
            ]]
        }

        // GPU
        if let gpuDevice = MTLCreateSystemDefaultDevice() {
            result["gpus"] = [[
                "name": gpuDevice.name,
                "vendor": "Apple",
                "vramMb": 0,
                "driverVersion": ""
            ]]
        }

        // Battery
        let wasMonitoring = device.isBatteryMonitoringEnabled
        device.isBatteryMonitoringEnabled = true
        let level = device.batteryLevel
        if level >= 0 {
            let status: String
            switch device.batteryState {
            case .charging: status = "charging"
            case .full: status = "full"
            case .unplugged: status = "discharging"
            default: status = "unknown"
            }
            result["battery"] = [
                "chargePercent": Double(level) * 100.0,
                "status": status,
                "healthPercent": NSNull(),
                "cycleCount": NSNull(),
                "designCapacityMwh": NSNull(),
                "fullChargeCapacityMwh": NSNull(),
                "timeToEmptySecs": NSNull(),
                "timeToFullSecs": NSNull()
            ] as [String: Any]
        } else {
            result["battery"] = NSNull()
        }
        device.isBatteryMonitoringEnabled = wasMonitoring

        // Networks
        var networks: [[String: Any]] = []
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        if getifaddrs(&ifaddr) == 0, let firstAddr = ifaddr {
            var interfaceMap: [String: [String: Any]] = [:]
            var ptr = firstAddr
            while true {
                let ifName = String(cString: ptr.pointee.ifa_name)
                let flags = Int32(ptr.pointee.ifa_flags)
                if (flags & IFF_LOOPBACK) == 0 {
                    if interfaceMap[ifName] == nil {
                        interfaceMap[ifName] = [
                            "name": ifName,
                            "macAddress": "00:00:00:00:00:00",
                            "ipv4": [String](),
                            "ipv6": [String](),
                            "rxBytes": 0,
                            "txBytes": 0,
                            "isUp": (flags & IFF_UP) != 0
                        ]
                    }
                    let family = ptr.pointee.ifa_addr.pointee.sa_family
                    if family == UInt8(AF_INET) {
                        var addr = ptr.pointee.ifa_addr.withMemoryRebound(to: sockaddr_in.self, capacity: 1) { $0.pointee.sin_addr }
                        var buffer = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
                        inet_ntop(AF_INET, &addr, &buffer, socklen_t(INET_ADDRSTRLEN))
                        var ipv4List = interfaceMap[ifName]!["ipv4"] as! [String]
                        ipv4List.append(String(cString: buffer))
                        interfaceMap[ifName]!["ipv4"] = ipv4List
                    } else if family == UInt8(AF_INET6) {
                        var addr = ptr.pointee.ifa_addr.withMemoryRebound(to: sockaddr_in6.self, capacity: 1) { $0.pointee.sin6_addr }
                        var buffer = [CChar](repeating: 0, count: Int(INET6_ADDRSTRLEN))
                        inet_ntop(AF_INET6, &addr, &buffer, socklen_t(INET6_ADDRSTRLEN))
                        var ipv6List = interfaceMap[ifName]!["ipv6"] as! [String]
                        ipv6List.append(String(cString: buffer))
                        interfaceMap[ifName]!["ipv6"] = ipv6List
                    }
                }
                guard let next = ptr.pointee.ifa_next else { break }
                ptr = next
            }
            freeifaddrs(ifaddr)
            networks = Array(interfaceMap.values)
        }
        result["networks"] = networks

        // Thermals
        let thermalState = processInfo.thermalState
        let thermalTemp: Double
        let thermalLabel: String
        switch thermalState {
        case .nominal: thermalTemp = 25.0; thermalLabel = "Device Thermal (Nominal)"
        case .fair: thermalTemp = 35.0; thermalLabel = "Device Thermal (Fair)"
        case .serious: thermalTemp = 45.0; thermalLabel = "Device Thermal (Serious)"
        case .critical: thermalTemp = 55.0; thermalLabel = "Device Thermal (Critical)"
        @unknown default: thermalTemp = 25.0; thermalLabel = "Device Thermal (Unknown)"
        }
        result["thermals"] = [[
            "label": thermalLabel,
            "temperatureCelsius": thermalTemp,
            "criticalCelsius": 80.0
        ]]

        // Displays
        let screen = UIScreen.main
        let bounds = screen.nativeBounds
        result["displays"] = [[
            "name": "Built-in Display",
            "width": Int(bounds.width),
            "height": Int(bounds.height),
            "dpi": Double(screen.nativeScale) * 163.0,
            "refreshRateHz": Double(screen.maximumFramesPerSecond),
            "isPrimary": true
        ]]

        invoke.resolve(result)
    }

    // MARK: - Helpers

    private func sysctlString(_ name: String) -> String? {
        var size: Int = 0
        sysctlbyname(name, nil, &size, nil, 0)
        guard size > 0 else { return nil }
        var buffer = [CChar](repeating: 0, count: size)
        sysctlbyname(name, &buffer, &size, nil, 0)
        return String(cString: buffer)
    }
}

@_cdecl("init_plugin_system")
func initPlugin() -> Plugin {
    return SystemPlugin()
}
