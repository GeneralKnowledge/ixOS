// Forge native services: device status (optional: `native_services!(device_status)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.

import Foundation
import os

#if canImport(UIKit)
    import UIKit
#endif

extension ForgeServices {
    static func installDeviceStatus() throws {
        installNativeDeviceStatus(provider: ForgeDeviceStatus.shared)
    }
}

/// `DeviceStatusProvider` over `UIDevice`, `UIScreen`, `ProcessInfo`, the
/// file system and the kernel's memory counters. UIKit values are read on
/// the main thread when they change and kept here, so Rust can ask from any
/// thread.
public final class ForgeDeviceStatus: DeviceStatusProvider, @unchecked Sendable {
    public static let shared = ForgeDeviceStatus()

    private let lock = NSLock()
    private var sink: DeviceStatusSink?
    private var deviceInfo: DeviceInfo
    private var batteryLevel: Float?
    private var batteryState: BatteryState = .unknown
    private var brightness: Float?
    private var observers: [NSObjectProtocol] = []

    private init() {
        deviceInfo = Self.processInfo()
        if Thread.isMainThread {
            MainActor.assumeIsolated { self.readUIKit() }
        } else {
            DispatchQueue.main.async { self.readUIKit() }
        }
        observeChanges()
    }

    public func info() -> DeviceInfo {
        lock.lock()
        defer { lock.unlock() }
        return deviceInfo
    }

    public func status() -> DeviceStatus {
        lock.lock()
        let level = batteryLevel
        let state = batteryState
        let screenBrightness = brightness
        lock.unlock()
        let process = ProcessInfo.processInfo
        let storage = Self.storage()
        return DeviceStatus(
            batteryLevel: level,
            batteryState: state,
            lowPowerMode: process.isLowPowerModeEnabled,
            thermalState: Self.thermal(process.thermalState),
            storageTotalBytes: storage.total,
            storageAvailableBytes: storage.available,
            memoryAvailableBytes: Self.availableMemory(),
            memoryFootprintBytes: Self.memoryFootprint(),
            uptimeSeconds: process.systemUptime,
            screenBrightness: screenBrightness,
            observedAtMs: Int64(Date().timeIntervalSince1970 * 1000))
    }

    public func start(sink: DeviceStatusSink) {
        lock.lock()
        self.sink = sink
        lock.unlock()
        sink.send(status: status())
    }

    public func stop() {
        lock.lock()
        sink = nil
        lock.unlock()
    }

    // MARK: Changes

    private func observeChanges() {
        var names: [Notification.Name] = [
            ProcessInfo.thermalStateDidChangeNotification,
            .NSProcessInfoPowerStateDidChange,
        ]
        #if os(iOS)
            names += [
                UIDevice.batteryLevelDidChangeNotification,
                UIDevice.batteryStateDidChangeNotification,
                UIScreen.brightnessDidChangeNotification,
            ]
        #endif
        let center = NotificationCenter.default
        observers = names.map { name in
            center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.changed() }
            }
        }
    }

    @MainActor
    private func changed() {
        readUIKit()
        lock.lock()
        let sink = self.sink
        lock.unlock()
        sink?.send(status: status())
    }

    @MainActor
    private func readUIKit() {
        #if os(iOS)
            let device = UIDevice.current
            device.isBatteryMonitoringEnabled = true
            let level = device.batteryLevel
            let state: BatteryState
            switch device.batteryState {
            case .unplugged: state = .unplugged
            case .charging: state = .charging
            case .full: state = .full
            default: state = .unknown
            }
            let screen = UIScreen.main
            var info = deviceInfo
            info.model = device.model
            info.systemName = device.systemName
            info.systemVersion = device.systemVersion
            info.deviceName = device.name
            info.screenWidthPoints = Double(screen.bounds.width)
            info.screenHeightPoints = Double(screen.bounds.height)
            info.screenScale = Double(screen.scale)
            lock.lock()
            batteryLevel = level >= 0 ? level : nil
            batteryState = state
            brightness = Float(screen.brightness)
            deviceInfo = info
            lock.unlock()
        #endif
    }

    // MARK: Readings

    /// What ProcessInfo knows; UIKit fills in the rest on iOS.
    static func processInfo() -> DeviceInfo {
        let process = ProcessInfo.processInfo
        let version = process.operatingSystemVersion
        #if targetEnvironment(simulator)
            let simulator = true
        #else
            let simulator = false
        #endif
        #if os(iOS)
            let systemName = "iOS"
        #else
            let systemName = "macOS"
        #endif
        return DeviceInfo(
            modelIdentifier: modelIdentifier(),
            model: systemName == "iOS" ? "iPhone" : "Mac",
            systemName: systemName,
            systemVersion: "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)",
            deviceName: systemName == "iOS" ? "iPhone" : "Mac",
            processorCount: UInt32(process.processorCount),
            activeProcessorCount: UInt32(process.activeProcessorCount),
            physicalMemoryBytes: process.physicalMemory,
            screenWidthPoints: 0,
            screenHeightPoints: 0,
            screenScale: 1,
            isSimulator: simulator,
            isIosAppOnMac: process.isiOSAppOnMac)
    }

    /// e.g. `iPhone16,2`. The simulator reports the Mac's architecture, so
    /// use the model it simulates.
    static func modelIdentifier() -> String {
        if let simulated = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] {
            return simulated
        }
        var size = 0
        let key = "hw.machine"
        guard sysctlbyname(key, nil, &size, nil, 0) == 0, size > 0 else { return "unknown" }
        var bytes = [CChar](repeating: 0, count: size)
        guard sysctlbyname(key, &bytes, &size, nil, 0) == 0 else { return "unknown" }
        return String(cString: bytes)
    }

    static func thermal(_ state: ProcessInfo.ThermalState) -> ThermalState {
        switch state {
        case .nominal: return .nominal
        case .fair: return .fair
        case .serious: return .serious
        case .critical: return .critical
        @unknown default: return .nominal
        }
    }

    static func storage() -> (total: UInt64?, available: UInt64?) {
        let home = URL(fileURLWithPath: NSHomeDirectory())
        guard
            let values = try? home.resourceValues(forKeys: [
                .volumeTotalCapacityKey, .volumeAvailableCapacityForImportantUsageKey,
            ])
        else { return (nil, nil) }
        let total = values.volumeTotalCapacity.map { UInt64(max(0, $0)) }
        let available = values.volumeAvailableCapacityForImportantUsage.map { UInt64(max(0, $0)) }
        return (total, available)
    }

    /// How much more this app may allocate before iOS ends it.
    static func availableMemory() -> UInt64? {
        #if os(iOS)
            let bytes = os_proc_available_memory()
            return bytes > 0 ? UInt64(bytes) : nil
        #else
            return nil
        #endif
    }

    /// The app's physical footprint: the number iOS compares to its limit.
    static func memoryFootprint() -> UInt64? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(
            MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let result = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { raw in
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), raw, &count)
            }
        }
        return result == KERN_SUCCESS ? UInt64(info.phys_footprint) : nil
    }
}
