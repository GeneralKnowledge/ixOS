import Foundation
import Observation

/// Spatial Sensor Lab: the phone's orientation and the magnetic field around
/// it, from one Core Motion device-motion stream (`SensorLabMotion.swift`).
///
/// A self-contained Device Lab instrument, not a framework capability: the
/// sensors are read in Swift and the rules here are small (a baseline and the
/// distance from it). This file imports Foundation and Observation only, so
/// the rules are tested without Core Motion.
///
/// What it measures is a field anomaly: how far the field has moved from the
/// baseline you set. A magnetometer can't tell how far away a magnet is, or
/// what an object is, so nothing here claims to.

/// A vector in three dimensions: device axes (x right, y towards the top, z
/// out of the screen) or the world's (x magnetic north, y west, z up).
public struct SensorVector: Equatable, Sendable {
    public var x: Double
    public var y: Double
    public var z: Double

    public init(_ x: Double, _ y: Double, _ z: Double) {
        self.x = x
        self.y = y
        self.z = z
    }

    public static let zero = SensorVector(0, 0, 0)

    public var length: Double { (x * x + y * y + z * z).squareRoot() }

    public static func + (a: Self, b: Self) -> Self { Self(a.x + b.x, a.y + b.y, a.z + b.z) }
    public static func - (a: Self, b: Self) -> Self { Self(a.x - b.x, a.y - b.y, a.z - b.z) }
    public static func * (a: Self, k: Double) -> Self { Self(a.x * k, a.y * k, a.z * k) }

    public func dot(_ b: Self) -> Double { x * b.x + y * b.y + z * b.z }

    public func cross(_ b: Self) -> Self {
        Self(y * b.z - z * b.y, z * b.x - x * b.z, x * b.y - y * b.x)
    }
}

/// The phone's attitude as Core Motion's rotation matrix, `m<row><column>`.
/// Core Motion's matrix turns world vectors into device vectors, so its
/// transpose turns device vectors into world ones (`toWorld`).
public struct SensorRotation: Equatable, Sendable {
    public var m11, m12, m13: Double
    public var m21, m22, m23: Double
    public var m31, m32, m33: Double

    public init(
        m11: Double, m12: Double, m13: Double,
        m21: Double, m22: Double, m23: Double,
        m31: Double, m32: Double, m33: Double
    ) {
        (self.m11, self.m12, self.m13) = (m11, m12, m13)
        (self.m21, self.m22, self.m23) = (m21, m22, m23)
        (self.m31, self.m32, self.m33) = (m31, m32, m33)
    }

    /// Lying flat, screen up, top of the phone towards magnetic north.
    public static let identity = SensorRotation(
        m11: 1, m12: 0, m13: 0, m21: 0, m22: 1, m23: 0, m31: 0, m32: 0, m33: 1)

    /// A device-frame vector in world axes.
    public func toWorld(_ v: SensorVector) -> SensorVector {
        SensorVector(
            m11 * v.x + m21 * v.y + m31 * v.z,
            m12 * v.x + m22 * v.y + m32 * v.z,
            m13 * v.x + m23 * v.y + m33 * v.z)
    }

    /// A world vector in device axes.
    public func toDevice(_ v: SensorVector) -> SensorVector {
        SensorVector(
            m11 * v.x + m12 * v.y + m13 * v.z,
            m21 * v.x + m22 * v.y + m23 * v.z,
            m31 * v.x + m32 * v.y + m33 * v.z)
    }
}

/// How far Core Motion trusts its magnetic-field calibration.
public enum FieldCalibration: Int, Comparable, Sendable {
    case uncalibrated, low, medium, high

    public static func < (a: Self, b: Self) -> Bool { a.rawValue < b.rawValue }

    public var name: String {
        switch self {
        case .uncalibrated: "Calibrating"
        case .low: "Low"
        case .medium: "Medium"
        case .high: "High"
        }
    }
}

/// One device-motion reading, copied out of Core Motion.
public struct SensorLabSample: Equatable, Sendable {
    /// Seconds, from Core Motion's clock (only differences matter).
    public var timestamp: Double
    public var rotation: SensorRotation
    /// Radians.
    public var roll: Double
    public var pitch: Double
    public var yaw: Double
    /// In g, device axes.
    public var gravity: SensorVector
    public var userAcceleration: SensorVector
    /// Calibrated magnetic field in µT, device axes; nil if the device has
    /// no magnetometer or the stream isn't magnetic-north referenced.
    public var field: SensorVector?
    public var calibration: FieldCalibration

    public init(
        timestamp: Double, rotation: SensorRotation, roll: Double, pitch: Double, yaw: Double,
        gravity: SensorVector, userAcceleration: SensorVector, field: SensorVector?,
        calibration: FieldCalibration
    ) {
        self.timestamp = timestamp
        self.rotation = rotation
        self.roll = roll
        self.pitch = pitch
        self.yaw = yaw
        self.gravity = gravity
        self.userAcceleration = userAcceleration
        self.field = field
        self.calibration = calibration
    }

    /// The field in world axes, which stays put as the phone turns, so a
    /// change in it is a change in the field, not in the phone.
    public var worldField: SensorVector? { field.map(rotation.toWorld) }
}

/// What the motion source can do on this device.
public struct SensorLabAvailability: Equatable, Sendable {
    public var deviceMotion: Bool
    /// Whether the magnetic-north reference frame (and so the calibrated
    /// field) is available.
    public var magneticNorth: Bool

    public init(deviceMotion: Bool, magneticNorth: Bool) {
        self.deviceMotion = deviceMotion
        self.magneticNorth = magneticNorth
    }
}

/// Where the samples come from: Core Motion on the phone, a script in tests.
@MainActor
public protocol SensorLabSource: AnyObject {
    var availability: SensorLabAvailability { get }
    /// Starts delivering samples on the main actor; `failed` gets Core
    /// Motion's error in words.
    func start(onSample: @escaping @Sendable @MainActor (SensorLabSample) -> Void, failed: @escaping @Sendable @MainActor (String) -> Void)
    func stop()
}

/// A baseline: the field in world axes, averaged over a moment.
public struct FieldBaseline: Equatable, Sendable {
    public var field: SensorVector
    public var strength: Double { field.length }
}

@MainActor
@Observable
public final class SensorLabModel {
    public enum Phase: Equatable, Sendable {
        case stopped
        case running
        case unavailable(String)
        case failed(String)
    }

    /// Samples averaged into a baseline: about half a second at 60 Hz.
    public static let baselineSamples = 30
    /// Samples between two points on the anomaly history: 10 a second.
    public static let historyStride = 6
    /// Points on the anomaly history: the last 30 seconds.
    public static let historyLength = 300
    /// The sampling interval asked of Core Motion.
    public static let interval: TimeInterval = 1.0 / 60

    public private(set) var phase: Phase = .stopped
    public private(set) var sample: SensorLabSample?
    public private(set) var baseline: FieldBaseline?
    /// Non-nil while a baseline is being collected: how many samples so far.
    public private(set) var collecting: Int?
    /// The anomaly (µT) ten times a second, oldest first; empty without a
    /// baseline.
    public private(set) var history: [Double] = []
    /// The largest anomaly since the baseline was set.
    public private(set) var peakAnomaly: Double?
    /// Measured sample rate.
    public private(set) var rateHz: Double = 0

    @ObservationIgnored private let source: any SensorLabSource
    @ObservationIgnored private var collected: [SensorVector] = []
    @ObservationIgnored private var sinceHistory = 0
    @ObservationIgnored private var rateWindow: [Double] = []

    public init(source: any SensorLabSource) {
        self.source = source
    }

    public var availability: SensorLabAvailability { source.availability }

    // MARK: Lifecycle

    /// Starts the stream (the screen appeared, or the app came back).
    public func start() {
        guard phase != .running else { return }
        let available = source.availability
        guard available.deviceMotion else {
            phase = .unavailable("This device has no motion sensors.")
            return
        }
        phase = .running
        rateWindow = []
        source.start(
            onSample: { [weak self] in self?.receive($0) },
            failed: { [weak self] in self?.fail($0) })
    }

    /// Stops the stream (the screen went away, or the app went to the
    /// background). The baseline stays.
    public func stop() {
        guard phase == .running else { return }
        source.stop()
        phase = .stopped
        collected = []
        collecting = nil
    }

    private func fail(_ reason: String) {
        source.stop()
        phase = .failed(reason)
    }

    // MARK: Readings

    public var strength: Double? { sample?.field?.length }

    /// How far the field (world axes) is from the baseline, in µT.
    public var anomaly: Double? {
        guard let baseline, let field = sample?.worldField else { return nil }
        return (field - baseline.field).length
    }

    /// Change in strength alone, in µT: positive is stronger than the
    /// baseline.
    public var strengthChange: Double? {
        guard let baseline, let strength else { return nil }
        return strength - baseline.strength
    }

    /// The field's angle below the horizon (positive pointing down, as in
    /// the northern hemisphere), in degrees.
    public var inclinationDegrees: Double? {
        guard let w = sample?.worldField, w.length > 0 else { return nil }
        return asin(-w.z / w.length) * 180 / .pi
    }

    /// Whether the field readings mean anything yet.
    public var fieldReady: Bool {
        guard let s = sample, s.field != nil else { return false }
        return s.calibration > .uncalibrated
    }

    // MARK: Baseline

    /// Averages the next `baselineSamples` readings into a baseline. Hold
    /// the phone away from magnets and metal while it collects.
    public func setBaseline() {
        guard phase == .running, fieldReady else { return }
        collected = []
        collecting = 0
    }

    public func clearBaseline() {
        baseline = nil
        collected = []
        collecting = nil
        history = []
        peakAnomaly = nil
    }

    /// Takes one sample. Public so tests (and the source) drive it.
    public func receive(_ next: SensorLabSample) {
        measureRate(next.timestamp)
        sample = next

        if collecting != nil, let field = next.worldField, next.calibration > .uncalibrated {
            collected.append(field)
            collecting = collected.count
            if collected.count >= Self.baselineSamples {
                let sum = collected.reduce(SensorVector.zero, +)
                baseline = FieldBaseline(field: sum * (1 / Double(collected.count)))
                collected = []
                collecting = nil
                history = []
                peakAnomaly = nil
                sinceHistory = 0
                // The anomaly starts from the next sample.
                return
            }
        }

        guard let anomaly else { return }
        peakAnomaly = max(peakAnomaly ?? 0, anomaly)
        sinceHistory += 1
        if sinceHistory >= Self.historyStride || history.isEmpty {
            sinceHistory = 0
            history.append(anomaly)
            if history.count > Self.historyLength {
                history.removeFirst(history.count - Self.historyLength)
            }
        }
    }

    private func measureRate(_ timestamp: Double) {
        rateWindow.append(timestamp)
        if rateWindow.count > 60 { rateWindow.removeFirst() }
        if let first = rateWindow.first, let last = rateWindow.last, last > first {
            rateHz = Double(rateWindow.count - 1) / (last - first)
        }
    }

    // MARK: Words

    /// A short reading of the anomaly for people, never a distance or an
    /// object: the magnetometer can tell neither.
    public static func describe(anomaly: Double) -> String {
        switch anomaly {
        case ..<3: "Steady"
        case ..<10: "Slight change"
        case ..<50: "Clear disturbance"
        default: "Strong disturbance"
        }
    }
}
