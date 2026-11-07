@preconcurrency import ForgeFFI
import Foundation

/// How values read on screen. All user-facing text lives in the Swift host;
/// Rust hands over numbers and enums.
enum Format {
    static func number(_ value: Double, digits: Int = 2) -> String {
        value.formatted(.number.precision(.fractionLength(digits)))
    }

    static func signed(_ value: Double, digits: Int = 2) -> String {
        value.formatted(.number.precision(.fractionLength(digits)).sign(strategy: .always()))
    }

    static func percent(_ fraction: Double) -> String {
        fraction.formatted(.percent.precision(.fractionLength(0)))
    }

    /// Storage sizes (decimal, as Settings shows them).
    static func storage(_ bytes: UInt64) -> String {
        Int64(clamping: bytes).formatted(.byteCount(style: .file))
    }

    /// Memory sizes (binary).
    static func memory(_ bytes: UInt64) -> String {
        Int64(clamping: bytes).formatted(.byteCount(style: .memory))
    }

    static func degrees(_ value: Double, digits: Int = 0) -> String {
        "\(number(value, digits: digits))°"
    }

    static func radiansAsDegrees(_ radians: Double) -> String {
        degrees(radians * 180 / .pi)
    }

    static func metres(_ value: Double, digits: Int = 0) -> String {
        Measurement(value: value, unit: UnitLength.meters)
            .formatted(.measurement(width: .abbreviated, usage: .asProvided, numberFormatStyle: .number.precision(.fractionLength(digits))))
    }

    static func distance(_ metres: Double) -> String {
        Measurement(value: metres, unit: UnitLength.meters)
            .formatted(.measurement(width: .abbreviated, usage: .road))
    }

    static func speed(_ mps: Double) -> String {
        Measurement(value: mps, unit: UnitSpeed.metersPerSecond)
            .formatted(.measurement(width: .abbreviated, usage: .general, numberFormatStyle: .number.precision(.fractionLength(1))))
    }

    static func pressure(_ kpa: Double) -> String {
        Measurement(value: kpa * 10, unit: UnitPressure.hectopascals)
            .formatted(.measurement(width: .abbreviated, usage: .asProvided, numberFormatStyle: .number.precision(.fractionLength(1))))
    }

    static func uptime(_ seconds: Double) -> String {
        Duration.seconds(seconds).formatted(
            .units(allowed: [.days, .hours, .minutes], width: .abbreviated, maximumUnitCount: 2))
    }

    static func time(unixMs: Int64) -> String {
        Date(timeIntervalSince1970: TimeInterval(unixMs) / 1000)
            .formatted(date: .omitted, time: .standard)
    }

    static func coordinate(_ value: Double, positive: String, negative: String) -> String {
        "\(number(abs(value), digits: 5))° \(value >= 0 ? positive : negative)"
    }

    /// N, NE, E, ... for a heading in degrees.
    static func cardinal(_ degrees: Double) -> String {
        let points = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
        let index = Int((degrees.truncatingRemainder(dividingBy: 360) / 45).rounded()) % 8
        return points[(index + 8) % 8]
    }

    static func vector(_ v: Vector3, digits: Int = 2) -> String {
        "x \(signed(v.x, digits: digits))  y \(signed(v.y, digits: digits))  z \(signed(v.z, digits: digits))"
    }
}

// MARK: Names

extension NetworkInterface {
    var name: String {
        switch self {
        case .wifi: "Wi-Fi"
        case .cellular: "Cellular"
        case .wired: "Wired"
        case .loopback: "Loopback"
        case .other: "Other"
        }
    }

    var symbol: String {
        switch self {
        case .wifi: "wifi"
        case .cellular: "antenna.radiowaves.left.and.right"
        case .wired: "cable.connector"
        default: "network"
        }
    }
}

extension ConnectivityStatus {
    var summary: String {
        switch reachability {
        case .online:
            if let first = interfaces.first { return "Online via \(first.name)" }
            return "Online"
        case .offline: return "Offline"
        case .requiresConnection: return "Waiting to Connect"
        case .unknown: return "Checking…"
        }
    }

    var symbol: String {
        switch reachability {
        case .online: interfaces.first?.symbol ?? "network"
        case .offline: "wifi.slash"
        default: "network"
        }
    }
}

extension OfflineReason {
    var name: String {
        switch self {
        case .notAvailable: "No network available"
        case .cellularDenied: "Mobile data is off for this app"
        case .wifiDenied: "Wi-Fi is off for this app"
        case .localNetworkDenied: "Local network access refused"
        case .other: "Unknown reason"
        }
    }
}

extension BluetoothState {
    var name: String {
        switch self {
        case .unknown: "Unknown"
        case .resetting: "Resetting"
        case .unsupported: "Not Supported"
        case .unauthorized: "Not Allowed"
        case .poweredOff: "Off"
        case .poweredOn: "On"
        }
    }
}

extension BatteryState {
    var name: String {
        switch self {
        case .unknown: "Unknown"
        case .unplugged: "On Battery"
        case .charging: "Charging"
        case .full: "Full"
        }
    }
}

extension ThermalState {
    var name: String {
        switch self {
        case .nominal: "Normal"
        case .fair: "Warm"
        case .serious: "Hot"
        case .critical: "Critical"
        }
    }
}

extension DeferReason {
    var name: String {
        switch self {
        case .thermal: "Would wait: the device is hot"
        case .lowPowerMode: "Would wait: Low Power Mode is on"
        case .lowBattery: "Would wait: battery under 20%"
        case .lowMemory: "Would wait: not enough memory"
        }
    }
}

extension MagneticAccuracy {
    var name: String {
        switch self {
        case .uncalibrated: "Uncalibrated"
        case .low: "Low"
        case .medium: "Medium"
        case .high: "High"
        }
    }
}

extension PermissionStatus {
    var allowsUse: Bool { self == .granted || self == .provisional }
}
