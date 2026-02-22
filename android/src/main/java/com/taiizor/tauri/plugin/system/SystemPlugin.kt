package com.taiizor.tauri.plugin.system

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.TrafficStats
import android.opengl.EGL14
import android.opengl.EGLConfig
import android.opengl.EGLContext
import android.opengl.EGLDisplay
import android.opengl.EGLSurface
import android.opengl.GLES20
import android.os.BatteryManager
import android.os.Build
import android.os.Environment
import android.os.PowerManager
import android.os.StatFs
import android.os.SystemClock
import android.util.DisplayMetrics
import android.view.WindowManager
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedReader
import java.io.File
import java.io.FileReader
import java.net.NetworkInterface
import java.util.Collections

@TauriPlugin
class SystemPlugin(private val activity: Activity) : Plugin(activity) {

    // ==================== OS ====================

    @Command
    fun get_os_info(invoke: Invoke) {
        try {
            val result = JSObject()
            result.put("name", "Android")
            result.put("version", Build.VERSION.RELEASE)
            result.put("fullVersion", "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
            result.put("hostname", Build.MODEL)
            result.put("arch", Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown")
            result.put("uptimeSecs", SystemClock.elapsedRealtime() / 1000)
            result.put("username", Build.MANUFACTURER)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get OS info: ${e.message}")
        }
    }

    // ==================== CPU ====================

    @Command
    fun get_cpu_info(invoke: Invoke) {
        try {
            val result = JSObject()
            var model = "Unknown"
            var vendor = "Unknown"

            try {
                BufferedReader(FileReader("/proc/cpuinfo")).use { reader ->
                    var line: String?
                    while (reader.readLine().also { line = it } != null) {
                        val parts = line!!.split(":").map { it.trim() }
                        if (parts.size == 2) {
                            when {
                                parts[0].equals("Hardware", ignoreCase = true) ||
                                parts[0].equals("model name", ignoreCase = true) -> model = parts[1]
                                parts[0].equals("CPU implementer", ignoreCase = true) -> {
                                    vendor = mapCpuImplementer(parts[1])
                                }
                            }
                        }
                    }
                }
            } catch (_: Exception) {}

            if (model == "Unknown") {
                model = Build.HARDWARE
            }

            val cores = Runtime.getRuntime().availableProcessors()
            result.put("model", model)
            result.put("vendor", vendor)
            result.put("cores", cores)
            result.put("threads", cores)
            result.put("arch", Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown")
            result.put("frequencyMhz", getMaxCpuFrequency())

            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get CPU info: ${e.message}")
        }
    }

    @Command
    fun get_cpu_usage(invoke: Invoke) {
        try {
            val cores = Runtime.getRuntime().availableProcessors()
            val usage1 = readPerCoreUsage(cores)
            Thread.sleep(200)
            val usage2 = readPerCoreUsage(cores)

            val result = JSObject()
            val usageArray = JSONArray()
            for (i in 0 until cores) {
                val totalDiff = usage2[i].total - usage1[i].total
                val idleDiff = usage2[i].idle - usage1[i].idle
                val pct = if (totalDiff > 0) {
                    ((totalDiff - idleDiff).toDouble() / totalDiff.toDouble()) * 100.0
                } else {
                    0.0
                }
                usageArray.put(pct.coerceIn(0.0, 100.0))
            }
            result.put("value", usageArray)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get CPU usage: ${e.message}")
        }
    }

    // ==================== Memory ====================

    @Command
    fun get_memory_info(invoke: Invoke) {
        try {
            val activityManager = activity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            val memInfo = ActivityManager.MemoryInfo()
            activityManager.getMemoryInfo(memInfo)

            var swapTotal = 0L
            var swapFree = 0L

            try {
                BufferedReader(FileReader("/proc/meminfo")).use { reader ->
                    var line: String?
                    while (reader.readLine().also { line = it } != null) {
                        when {
                            line!!.startsWith("SwapTotal:") -> {
                                swapTotal = parseMemInfoLine(line!!) * 1024
                            }
                            line!!.startsWith("SwapFree:") -> {
                                swapFree = parseMemInfoLine(line!!) * 1024
                            }
                        }
                    }
                }
            } catch (_: Exception) {}

            val result = JSObject()
            result.put("totalBytes", memInfo.totalMem)
            result.put("usedBytes", memInfo.totalMem - memInfo.availMem)
            result.put("freeBytes", memInfo.availMem)
            result.put("availableBytes", memInfo.availMem)
            result.put("swapTotalBytes", swapTotal)
            result.put("swapUsedBytes", swapTotal - swapFree)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get memory info: ${e.message}")
        }
    }

    // ==================== Disk ====================

    @Command
    fun get_disk_info(invoke: Invoke) {
        try {
            val result = JSObject()
            val disksArray = JSONArray()

            val stat = StatFs(Environment.getDataDirectory().path)
            val totalBytes = stat.blockSizeLong * stat.blockCountLong
            val freeBytes = stat.blockSizeLong * stat.availableBlocksLong
            val usedBytes = totalBytes - freeBytes

            val disk = JSONObject()
            disk.put("name", "Internal Storage")
            disk.put("mountPoint", "/data")
            disk.put("fsType", "ext4")
            disk.put("kind", "ssd")
            disk.put("totalBytes", totalBytes)
            disk.put("usedBytes", usedBytes)
            disk.put("freeBytes", freeBytes)
            disk.put("isRemovable", false)
            disksArray.put(disk)

            val externalDirs = activity.getExternalFilesDirs(null)
            for (i in 1 until externalDirs.size) {
                val dir = externalDirs[i] ?: continue
                try {
                    val extStat = StatFs(dir.path)
                    val extTotal = extStat.blockSizeLong * extStat.blockCountLong
                    val extFree = extStat.blockSizeLong * extStat.availableBlocksLong

                    val extDisk = JSONObject()
                    extDisk.put("name", "External Storage ${i}")
                    extDisk.put("mountPoint", dir.path)
                    extDisk.put("fsType", "unknown")
                    extDisk.put("kind", "unknown")
                    extDisk.put("totalBytes", extTotal)
                    extDisk.put("usedBytes", extTotal - extFree)
                    extDisk.put("freeBytes", extFree)
                    extDisk.put("isRemovable", true)
                    disksArray.put(extDisk)
                } catch (_: Exception) {}
            }

            result.put("value", disksArray)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get disk info: ${e.message}")
        }
    }

    // ==================== GPU ====================

    @Command
    fun get_gpu_info(invoke: Invoke) {
        try {
            val result = JSObject()
            val gpuArray = JSONArray()

            var renderer = "Unknown"
            var gpuVendor = "Unknown"

            var eglDisplay: EGLDisplay? = null
            var eglContext: EGLContext? = null
            var eglSurface: EGLSurface? = null

            try {
                eglDisplay = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
                val version = IntArray(2)
                EGL14.eglInitialize(eglDisplay, version, 0, version, 1)

                val configAttribs = intArrayOf(
                    EGL14.EGL_RENDERABLE_TYPE, EGL14.EGL_OPENGL_ES2_BIT,
                    EGL14.EGL_SURFACE_TYPE, EGL14.EGL_PBUFFER_BIT,
                    EGL14.EGL_NONE
                )
                val configs = arrayOfNulls<EGLConfig>(1)
                val numConfigs = IntArray(1)
                EGL14.eglChooseConfig(eglDisplay, configAttribs, 0, configs, 0, 1, numConfigs, 0)

                val contextAttribs = intArrayOf(
                    EGL14.EGL_CONTEXT_CLIENT_VERSION, 2,
                    EGL14.EGL_NONE
                )
                eglContext = EGL14.eglCreateContext(
                    eglDisplay, configs[0], EGL14.EGL_NO_CONTEXT, contextAttribs, 0
                )

                val surfaceAttribs = intArrayOf(
                    EGL14.EGL_WIDTH, 1,
                    EGL14.EGL_HEIGHT, 1,
                    EGL14.EGL_NONE
                )
                eglSurface = EGL14.eglCreatePbufferSurface(eglDisplay, configs[0], surfaceAttribs, 0)

                EGL14.eglMakeCurrent(eglDisplay, eglSurface, eglSurface, eglContext)

                renderer = GLES20.glGetString(GLES20.GL_RENDERER) ?: "Unknown"
                gpuVendor = GLES20.glGetString(GLES20.GL_VENDOR) ?: "Unknown"
            } catch (_: Exception) {
            } finally {
                try {
                    if (eglDisplay != null) {
                        EGL14.eglMakeCurrent(
                            eglDisplay, EGL14.EGL_NO_SURFACE,
                            EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_CONTEXT
                        )
                        if (eglSurface != null) EGL14.eglDestroySurface(eglDisplay, eglSurface)
                        if (eglContext != null) EGL14.eglDestroyContext(eglDisplay, eglContext)
                        EGL14.eglTerminate(eglDisplay)
                    }
                } catch (_: Exception) {}
            }

            val gpu = JSONObject()
            gpu.put("name", renderer)
            gpu.put("vendor", gpuVendor)
            gpu.put("vramMb", 0)
            gpu.put("driverVersion", "")
            gpuArray.put(gpu)

            result.put("value", gpuArray)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get GPU info: ${e.message}")
        }
    }

    // ==================== Battery ====================

    @Command
    fun get_battery_info(invoke: Invoke) {
        try {
            val intentFilter = IntentFilter(Intent.ACTION_BATTERY_CHANGED)
            val batteryStatus = activity.registerReceiver(null, intentFilter)

            if (batteryStatus == null) {
                val result = JSObject()
                result.put("value", JSObject.NULL)
                invoke.resolve(result)
                return
            }

            val level = batteryStatus.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val scale = batteryStatus.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
            val chargePercent = if (level >= 0 && scale > 0) {
                (level.toDouble() / scale.toDouble()) * 100.0
            } else {
                0.0
            }

            val plugged = batteryStatus.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
            val status = when (plugged) {
                BatteryManager.BATTERY_STATUS_CHARGING -> "charging"
                BatteryManager.BATTERY_STATUS_DISCHARGING -> "discharging"
                BatteryManager.BATTERY_STATUS_FULL -> "full"
                BatteryManager.BATTERY_STATUS_NOT_CHARGING -> "notCharging"
                else -> "unknown"
            }

            val healthInt = batteryStatus.getIntExtra(BatteryManager.EXTRA_HEALTH, -1)
            val healthPercent: Double? = when (healthInt) {
                BatteryManager.BATTERY_HEALTH_GOOD -> 100.0
                BatteryManager.BATTERY_HEALTH_OVERHEAT,
                BatteryManager.BATTERY_HEALTH_DEAD -> 0.0
                else -> null
            }

            val temperature = batteryStatus.getIntExtra(BatteryManager.EXTRA_TEMPERATURE, -1)

            val result = JSObject()
            result.put("chargePercent", chargePercent)
            result.put("status", status)
            if (healthPercent != null) {
                result.put("healthPercent", healthPercent)
            } else {
                result.put("healthPercent", JSObject.NULL)
            }
            result.put("cycleCount", JSObject.NULL)
            result.put("designCapacityMwh", JSObject.NULL)
            result.put("fullChargeCapacityMwh", JSObject.NULL)
            result.put("timeToEmptySecs", JSObject.NULL)
            result.put("timeToFullSecs", JSObject.NULL)

            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get battery info: ${e.message}")
        }
    }

    // ==================== Network ====================

    @Command
    fun get_network_info(invoke: Invoke) {
        try {
            val result = JSObject()
            val networksArray = JSONArray()

            val interfaces = Collections.list(NetworkInterface.getNetworkInterfaces())
            for (iface in interfaces) {
                if (iface.isLoopback) continue

                val network = JSONObject()
                network.put("name", iface.name)

                val macBytes = iface.hardwareAddress
                val mac = if (macBytes != null) {
                    macBytes.joinToString(":") { "%02x".format(it) }
                } else {
                    "00:00:00:00:00:00"
                }
                network.put("macAddress", mac)

                val ipv4List = JSONArray()
                val ipv6List = JSONArray()
                for (addr in iface.inetAddresses) {
                    val hostAddr = addr.hostAddress ?: continue
                    if (addr is java.net.Inet4Address) {
                        ipv4List.put(hostAddr)
                    } else if (addr is java.net.Inet6Address) {
                        ipv6List.put(hostAddr.split("%")[0])
                    }
                }
                network.put("ipv4", ipv4List)
                network.put("ipv6", ipv6List)

                val uid = android.os.Process.myUid()
                network.put("rxBytes", TrafficStats.getUidRxBytes(uid).coerceAtLeast(0))
                network.put("txBytes", TrafficStats.getUidTxBytes(uid).coerceAtLeast(0))
                network.put("isUp", iface.isUp)

                networksArray.put(network)
            }

            result.put("value", networksArray)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get network info: ${e.message}")
        }
    }

    // ==================== Thermal ====================

    @Command
    fun get_thermal_info(invoke: Invoke) {
        try {
            val result = JSObject()
            val thermalsArray = JSONArray()

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val powerManager = activity.getSystemService(Context.POWER_SERVICE) as PowerManager
                val thermalStatus = powerManager.currentThermalStatus

                val thermal = JSONObject()
                thermal.put("label", "Device Thermal Status")
                thermal.put("temperatureCelsius", mapThermalStatusToTemp(thermalStatus))
                thermal.put("criticalCelsius", 80.0)
                thermalsArray.put(thermal)
            }

            // Read from thermal zones if available
            try {
                val thermalDir = File("/sys/class/thermal")
                if (thermalDir.exists()) {
                    thermalDir.listFiles()?.filter { it.name.startsWith("thermal_zone") }?.forEach { zone ->
                        try {
                            val typeFile = File(zone, "type")
                            val tempFile = File(zone, "temp")
                            if (typeFile.exists() && tempFile.exists()) {
                                val label = typeFile.readText().trim()
                                val tempMilliC = tempFile.readText().trim().toLongOrNull() ?: return@forEach
                                val tempC = tempMilliC.toDouble() / 1000.0

                                if (tempC > 0 && tempC < 150) {
                                    val thermal = JSONObject()
                                    thermal.put("label", label)
                                    thermal.put("temperatureCelsius", tempC)
                                    thermal.put("criticalCelsius", JSONObject.NULL)
                                    thermalsArray.put(thermal)
                                }
                            }
                        } catch (_: Exception) {}
                    }
                }
            } catch (_: Exception) {}

            result.put("value", thermalsArray)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get thermal info: ${e.message}")
        }
    }

    // ==================== Display ====================

    @Command
    fun get_display_info(invoke: Invoke) {
        try {
            val result = JSObject()
            val displaysArray = JSONArray()

            val windowManager = activity.getSystemService(Context.WINDOW_SERVICE) as WindowManager
            val metrics = DisplayMetrics()

            @Suppress("DEPRECATION")
            val display = windowManager.defaultDisplay
            @Suppress("DEPRECATION")
            display.getRealMetrics(metrics)

            val displayObj = JSONObject()
            displayObj.put("name", "Built-in Display")
            displayObj.put("width", metrics.widthPixels)
            displayObj.put("height", metrics.heightPixels)
            displayObj.put("dpi", metrics.densityDpi.toDouble())

            @Suppress("DEPRECATION")
            val refreshRate = display.refreshRate.toDouble()
            displayObj.put("refreshRateHz", refreshRate)
            displayObj.put("isPrimary", true)
            displaysArray.put(displayObj)

            result.put("value", displaysArray)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get display info: ${e.message}")
        }
    }

    // ==================== All Info ====================

    @Command
    fun get_all_info(invoke: Invoke) {
        try {
            val result = JSObject()

            // OS
            try {
                val os = JSObject()
                os.put("name", "Android")
                os.put("version", Build.VERSION.RELEASE)
                os.put("fullVersion", "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
                os.put("hostname", Build.MODEL)
                os.put("arch", Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown")
                os.put("uptimeSecs", SystemClock.elapsedRealtime() / 1000)
                os.put("username", Build.MANUFACTURER)
                result.put("os", os)
            } catch (_: Exception) {
                result.put("os", JSObject.NULL)
            }

            // CPU
            try {
                val cpu = JSObject()
                var cpuModel = Build.HARDWARE
                var cpuVendor = "Unknown"
                try {
                    BufferedReader(FileReader("/proc/cpuinfo")).use { reader ->
                        var line: String?
                        while (reader.readLine().also { line = it } != null) {
                            val parts = line!!.split(":").map { it.trim() }
                            if (parts.size == 2) {
                                when {
                                    parts[0].equals("Hardware", ignoreCase = true) ||
                                    parts[0].equals("model name", ignoreCase = true) -> cpuModel = parts[1]
                                    parts[0].equals("CPU implementer", ignoreCase = true) ->
                                        cpuVendor = mapCpuImplementer(parts[1])
                                }
                            }
                        }
                    }
                } catch (_: Exception) {}

                val cores = Runtime.getRuntime().availableProcessors()
                cpu.put("model", cpuModel)
                cpu.put("vendor", cpuVendor)
                cpu.put("cores", cores)
                cpu.put("threads", cores)
                cpu.put("arch", Build.SUPPORTED_ABIS.firstOrNull() ?: "unknown")
                cpu.put("frequencyMhz", getMaxCpuFrequency())
                result.put("cpu", cpu)
            } catch (_: Exception) {
                result.put("cpu", JSObject.NULL)
            }

            // Memory
            try {
                val activityManager = activity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
                val memInfo = ActivityManager.MemoryInfo()
                activityManager.getMemoryInfo(memInfo)

                val mem = JSObject()
                mem.put("totalBytes", memInfo.totalMem)
                mem.put("usedBytes", memInfo.totalMem - memInfo.availMem)
                mem.put("freeBytes", memInfo.availMem)
                mem.put("availableBytes", memInfo.availMem)
                mem.put("swapTotalBytes", 0)
                mem.put("swapUsedBytes", 0)
                result.put("memory", mem)
            } catch (_: Exception) {
                result.put("memory", JSObject.NULL)
            }

            // Disks
            try {
                val disksArray = JSONArray()
                val stat = StatFs(Environment.getDataDirectory().path)
                val totalBytes = stat.blockSizeLong * stat.blockCountLong
                val freeBytes = stat.blockSizeLong * stat.availableBlocksLong

                val disk = JSONObject()
                disk.put("name", "Internal Storage")
                disk.put("mountPoint", "/data")
                disk.put("fsType", "ext4")
                disk.put("kind", "ssd")
                disk.put("totalBytes", totalBytes)
                disk.put("usedBytes", totalBytes - freeBytes)
                disk.put("freeBytes", freeBytes)
                disk.put("isRemovable", false)
                disksArray.put(disk)
                result.put("disks", disksArray)
            } catch (_: Exception) {
                result.put("disks", JSObject.NULL)
            }

            // GPU
            try {
                val gpuArray = JSONArray()
                val gpu = JSONObject()
                gpu.put("name", Build.HARDWARE)
                gpu.put("vendor", Build.MANUFACTURER)
                gpu.put("vramMb", 0)
                gpu.put("driverVersion", "")
                gpuArray.put(gpu)
                result.put("gpus", gpuArray)
            } catch (_: Exception) {
                result.put("gpus", JSObject.NULL)
            }

            // Battery
            try {
                val intentFilter = IntentFilter(Intent.ACTION_BATTERY_CHANGED)
                val batteryStatus = activity.registerReceiver(null, intentFilter)
                if (batteryStatus != null) {
                    val level = batteryStatus.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
                    val scale = batteryStatus.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
                    val chargePercent = if (level >= 0 && scale > 0) {
                        (level.toDouble() / scale.toDouble()) * 100.0
                    } else { 0.0 }
                    val statusInt = batteryStatus.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
                    val status = when (statusInt) {
                        BatteryManager.BATTERY_STATUS_CHARGING -> "charging"
                        BatteryManager.BATTERY_STATUS_DISCHARGING -> "discharging"
                        BatteryManager.BATTERY_STATUS_FULL -> "full"
                        BatteryManager.BATTERY_STATUS_NOT_CHARGING -> "notCharging"
                        else -> "unknown"
                    }
                    val bat = JSObject()
                    bat.put("chargePercent", chargePercent)
                    bat.put("status", status)
                    bat.put("healthPercent", JSObject.NULL)
                    bat.put("cycleCount", JSObject.NULL)
                    bat.put("designCapacityMwh", JSObject.NULL)
                    bat.put("fullChargeCapacityMwh", JSObject.NULL)
                    bat.put("timeToEmptySecs", JSObject.NULL)
                    bat.put("timeToFullSecs", JSObject.NULL)
                    result.put("battery", bat)
                } else {
                    result.put("battery", JSObject.NULL)
                }
            } catch (_: Exception) {
                result.put("battery", JSObject.NULL)
            }

            // Networks
            try {
                val networksArray = JSONArray()
                val interfaces = Collections.list(NetworkInterface.getNetworkInterfaces())
                for (iface in interfaces) {
                    if (iface.isLoopback) continue
                    val network = JSONObject()
                    network.put("name", iface.name)
                    val macBytes = iface.hardwareAddress
                    network.put("macAddress", macBytes?.joinToString(":") { "%02x".format(it) } ?: "00:00:00:00:00:00")
                    val ipv4List = JSONArray()
                    val ipv6List = JSONArray()
                    for (addr in iface.inetAddresses) {
                        val hostAddr = addr.hostAddress ?: continue
                        if (addr is java.net.Inet4Address) ipv4List.put(hostAddr)
                        else if (addr is java.net.Inet6Address) ipv6List.put(hostAddr.split("%")[0])
                    }
                    network.put("ipv4", ipv4List)
                    network.put("ipv6", ipv6List)
                    network.put("rxBytes", TrafficStats.getTotalRxBytes().coerceAtLeast(0))
                    network.put("txBytes", TrafficStats.getTotalTxBytes().coerceAtLeast(0))
                    network.put("isUp", iface.isUp)
                    networksArray.put(network)
                }
                result.put("networks", networksArray)
            } catch (_: Exception) {
                result.put("networks", JSObject.NULL)
            }

            // Thermals
            try {
                val thermalsArray = JSONArray()
                val thermalDir = File("/sys/class/thermal")
                if (thermalDir.exists()) {
                    thermalDir.listFiles()?.filter { it.name.startsWith("thermal_zone") }?.forEach { zone ->
                        try {
                            val typeFile = File(zone, "type")
                            val tempFile = File(zone, "temp")
                            if (typeFile.exists() && tempFile.exists()) {
                                val label = typeFile.readText().trim()
                                val tempMilliC = tempFile.readText().trim().toLongOrNull() ?: return@forEach
                                val tempC = tempMilliC.toDouble() / 1000.0
                                if (tempC > 0 && tempC < 150) {
                                    val thermal = JSONObject()
                                    thermal.put("label", label)
                                    thermal.put("temperatureCelsius", tempC)
                                    thermal.put("criticalCelsius", JSONObject.NULL)
                                    thermalsArray.put(thermal)
                                }
                            }
                        } catch (_: Exception) {}
                    }
                }
                result.put("thermals", thermalsArray)
            } catch (_: Exception) {
                result.put("thermals", JSObject.NULL)
            }

            // Displays
            try {
                val displaysArray = JSONArray()
                val windowManager = activity.getSystemService(Context.WINDOW_SERVICE) as WindowManager
                val metrics = DisplayMetrics()
                @Suppress("DEPRECATION")
                val display = windowManager.defaultDisplay
                @Suppress("DEPRECATION")
                display.getRealMetrics(metrics)
                val displayObj = JSONObject()
                displayObj.put("name", "Built-in Display")
                displayObj.put("width", metrics.widthPixels)
                displayObj.put("height", metrics.heightPixels)
                displayObj.put("dpi", metrics.densityDpi.toDouble())
                @Suppress("DEPRECATION")
                displayObj.put("refreshRateHz", display.refreshRate.toDouble())
                displayObj.put("isPrimary", true)
                displaysArray.put(displayObj)
                result.put("displays", displaysArray)
            } catch (_: Exception) {
                result.put("displays", JSObject.NULL)
            }

            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get all info: ${e.message}")
        }
    }

    // ==================== Helpers ====================

    private data class CpuTick(val total: Long, val idle: Long)

    private fun readPerCoreUsage(cores: Int): List<CpuTick> {
        val ticks = mutableListOf<CpuTick>()
        try {
            BufferedReader(FileReader("/proc/stat")).use { reader ->
                reader.readLine() // skip total line
                for (i in 0 until cores) {
                    val line = reader.readLine() ?: break
                    if (!line.startsWith("cpu")) break
                    val parts = line.trim().split("\\s+".toRegex())
                    if (parts.size >= 8) {
                        val user = parts[1].toLongOrNull() ?: 0
                        val nice = parts[2].toLongOrNull() ?: 0
                        val system = parts[3].toLongOrNull() ?: 0
                        val idle = parts[4].toLongOrNull() ?: 0
                        val iowait = parts[5].toLongOrNull() ?: 0
                        val irq = parts[6].toLongOrNull() ?: 0
                        val softirq = parts[7].toLongOrNull() ?: 0
                        val total = user + nice + system + idle + iowait + irq + softirq
                        ticks.add(CpuTick(total, idle))
                    }
                }
            }
        } catch (_: Exception) {}

        while (ticks.size < cores) {
            ticks.add(CpuTick(0, 0))
        }
        return ticks
    }

    private fun getMaxCpuFrequency(): Long {
        try {
            val file = File("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
            if (file.exists()) {
                val khz = file.readText().trim().toLongOrNull() ?: return 0
                return khz / 1000
            }
        } catch (_: Exception) {}
        return 0
    }

    private fun parseMemInfoLine(line: String): Long {
        val parts = line.trim().split("\\s+".toRegex())
        return if (parts.size >= 2) parts[1].toLongOrNull() ?: 0 else 0
    }

    private fun mapCpuImplementer(code: String): String {
        return when (code.trim().lowercase()) {
            "0x41" -> "ARM"
            "0x42" -> "Broadcom"
            "0x43" -> "Cavium"
            "0x44" -> "DEC"
            "0x4e" -> "NVIDIA"
            "0x50" -> "APM"
            "0x51" -> "Qualcomm"
            "0x53" -> "Samsung"
            "0x56" -> "Marvell"
            "0x61" -> "Apple"
            "0x66" -> "Faraday"
            "0x69" -> "Intel"
            "0xc0" -> "Ampere"
            else -> "Unknown ($code)"
        }
    }

    private fun mapThermalStatusToTemp(status: Int): Double {
        return when (status) {
            0 -> 25.0  // THERMAL_STATUS_NONE
            1 -> 35.0  // THERMAL_STATUS_LIGHT
            2 -> 45.0  // THERMAL_STATUS_MODERATE
            3 -> 55.0  // THERMAL_STATUS_SEVERE
            4 -> 65.0  // THERMAL_STATUS_CRITICAL
            5 -> 75.0  // THERMAL_STATUS_EMERGENCY
            6 -> 85.0  // THERMAL_STATUS_SHUTDOWN
            else -> 25.0
        }
    }
}
