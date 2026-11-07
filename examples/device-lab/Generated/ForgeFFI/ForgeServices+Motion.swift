// Forge native services: motion sensors (optional: `native_services!(motion)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.
//
// The barometer needs the Motion & Fitness permission and an
// NSMotionUsageDescription (forge.toml `[ios.permissions] motion`). The
// accelerometer, gyroscope, magnetometer and device motion need neither.

import Foundation

#if os(iOS)
    import CoreMotion
#endif

extension ForgeServices {
    static func installMotion() throws {
        installNativeMotion(sensors: ForgeMotionSensors.shared)
        #if os(iOS)
            ForgePermissions.shared.register(MotionPermissionAdapter(), for: .motion)
        #endif
    }
}

#if os(iOS)

    /// `MotionSensors` backed by Core Motion. Updates arrive on one serial
    /// queue, off the main thread.
    public final class ForgeMotionSensors: MotionSensors, @unchecked Sendable {
        public static let shared = ForgeMotionSensors()

        private let manager = CMMotionManager()
        private let altimeter = CMAltimeter()
        private let queue: OperationQueue = {
            let queue = OperationQueue()
            queue.name = "org.forgemobile.services.motion"
            queue.maxConcurrentOperationCount = 1
            return queue
        }()

        private init() {}

        public func availability() -> MotionAvailability {
            MotionAvailability(
                accelerometer: manager.isAccelerometerAvailable,
                gyroscope: manager.isGyroAvailable,
                magnetometer: manager.isMagnetometerAvailable,
                deviceMotion: manager.isDeviceMotionAvailable,
                altimeter: CMAltimeter.isRelativeAltitudeAvailable())
        }

        public func start(sensor: MotionSensor, intervalMs: UInt32, sink: MotionSink) throws {
            let interval = TimeInterval(intervalMs) / 1000
            let available = availability()
            switch sensor {
            case .accelerometer:
                guard available.accelerometer else { throw Self.missing("accelerometer") }
                manager.accelerometerUpdateInterval = interval
                manager.startAccelerometerUpdates(to: queue) { data, error in
                    if let data {
                        let a = data.acceleration
                        sink.send(
                            reading: .acceleration(
                                value: Vector3(x: a.x, y: a.y, z: a.z), timestampS: data.timestamp))
                    } else if let error {
                        sink.send(reading: .failed(sensor: .accelerometer, reason: Self.describe(error)))
                    }
                }
            case .gyroscope:
                guard available.gyroscope else { throw Self.missing("gyroscope") }
                manager.gyroUpdateInterval = interval
                manager.startGyroUpdates(to: queue) { data, error in
                    if let data {
                        let r = data.rotationRate
                        sink.send(
                            reading: .rotationRate(
                                value: Vector3(x: r.x, y: r.y, z: r.z), timestampS: data.timestamp))
                    } else if let error {
                        sink.send(reading: .failed(sensor: .gyroscope, reason: Self.describe(error)))
                    }
                }
            case .magnetometer:
                guard available.magnetometer else { throw Self.missing("magnetometer") }
                manager.magnetometerUpdateInterval = interval
                manager.startMagnetometerUpdates(to: queue) { data, error in
                    if let data {
                        let m = data.magneticField
                        sink.send(
                            reading: .magneticField(
                                value: Vector3(x: m.x, y: m.y, z: m.z), timestampS: data.timestamp))
                    } else if let error {
                        sink.send(reading: .failed(sensor: .magnetometer, reason: Self.describe(error)))
                    }
                }
            case .deviceMotion:
                guard available.deviceMotion else { throw Self.missing("device motion") }
                manager.deviceMotionUpdateInterval = interval
                // A magnetic-north frame gives a heading and a calibrated
                // field; without a magnetometer, an arbitrary one.
                let magnetic = CMMotionManager.availableAttitudeReferenceFrames()
                    .contains(.xMagneticNorthZVertical)
                let frame: CMAttitudeReferenceFrame =
                    magnetic ? .xMagneticNorthZVertical : .xArbitraryZVertical
                manager.startDeviceMotionUpdates(using: frame, to: queue) { motion, error in
                    if let motion {
                        sink.send(
                            reading: .deviceMotion(
                                sample: Self.sample(motion, magnetic: magnetic),
                                timestampS: motion.timestamp))
                    } else if let error {
                        sink.send(reading: .failed(sensor: .deviceMotion, reason: Self.describe(error)))
                    }
                }
            case .altimeter:
                guard available.altimeter else { throw Self.missing("barometer") }
                guard Bundle.main.object(forInfoDictionaryKey: "NSMotionUsageDescription") != nil else {
                    throw NativeServiceError.Unavailable(
                        reason: "the barometer needs NSMotionUsageDescription: add [ios.permissions] motion to forge.toml")
                }
                // The altimeter picks its own rate (about once a second).
                altimeter.startRelativeAltitudeUpdates(to: queue) { data, error in
                    if let data {
                        sink.send(
                            reading: .altitude(
                                relativeAltitudeM: data.relativeAltitude.doubleValue,
                                pressureKpa: data.pressure.doubleValue,
                                timestampS: data.timestamp))
                    } else if let error {
                        sink.send(reading: .failed(sensor: .altimeter, reason: Self.describe(error)))
                    }
                }
            }
        }

        public func stop(sensor: MotionSensor) {
            switch sensor {
            case .accelerometer: manager.stopAccelerometerUpdates()
            case .gyroscope: manager.stopGyroUpdates()
            case .magnetometer: manager.stopMagnetometerUpdates()
            case .deviceMotion: manager.stopDeviceMotionUpdates()
            case .altimeter: altimeter.stopRelativeAltitudeUpdates()
            }
        }

        static func sample(_ motion: CMDeviceMotion, magnetic: Bool) -> DeviceMotionSample {
            let attitude = motion.attitude
            let g = motion.gravity
            let u = motion.userAcceleration
            let r = motion.rotationRate
            let field = motion.magneticField
            let accuracy: MagneticAccuracy
            switch field.accuracy {
            case .low: accuracy = .low
            case .medium: accuracy = .medium
            case .high: accuracy = .high
            default: accuracy = .uncalibrated
            }
            let calibrated = magnetic && accuracy != .uncalibrated
            return DeviceMotionSample(
                attitude: Attitude(roll: attitude.roll, pitch: attitude.pitch, yaw: attitude.yaw),
                gravity: Vector3(x: g.x, y: g.y, z: g.z),
                userAcceleration: Vector3(x: u.x, y: u.y, z: u.z),
                rotationRate: Vector3(x: r.x, y: r.y, z: r.z),
                magneticField: calibrated
                    ? Vector3(x: field.field.x, y: field.field.y, z: field.field.z) : nil,
                magneticAccuracy: accuracy,
                headingDegrees: calibrated && motion.heading >= 0 ? motion.heading : nil)
        }

        static func missing(_ name: String) -> NativeServiceError {
            NativeServiceError.Unavailable(reason: "this device has no \(name)")
        }

        static func describe(_ error: Error) -> String {
            let ns = error as NSError
            // CMErrorMotionActivityNotAuthorized
            if ns.domain == CMErrorDomain && ns.code == 105 {
                return "Motion & Fitness access is off for this app"
            }
            return ns.localizedDescription
        }
    }

    /// Motion & Fitness. Core Motion has no request call: the first query
    /// shows the prompt, and its answer arrives with the query's result.
    struct MotionPermissionAdapter: ForgePermissionAdapter {
        func status() async -> PermissionStatus {
            switch CMMotionActivityManager.authorizationStatus() {
            case .notDetermined: return .notDetermined
            case .restricted: return .restricted
            case .denied: return .denied
            case .authorized: return .granted
            @unknown default: return .unsupported
            }
        }

        func request() async -> PermissionStatus {
            let current = await status()
            guard current == .notDetermined else { return current }
            guard Bundle.main.object(forInfoDictionaryKey: "NSMotionUsageDescription") != nil else {
                print("[forge] motion permission: add [ios.permissions] motion to forge.toml first")
                return .unsupported
            }
            guard CMMotionActivityManager.isActivityAvailable() else { return .unsupported }
            await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
                let activity = CMMotionActivityManager()
                let now = Date()
                activity.queryActivityStarting(from: now.addingTimeInterval(-60), to: now, to: .main) { _, _ in
                    withExtendedLifetime(activity) { done.resume() }
                }
            }
            return await status()
        }
    }

#else

    /// Core Motion's sensors exist only on iOS; elsewhere (`swift test` on a
    /// Mac) every sensor reports unavailable.
    public final class ForgeMotionSensors: MotionSensors, @unchecked Sendable {
        public static let shared = ForgeMotionSensors()

        private init() {}

        public func availability() -> MotionAvailability {
            MotionAvailability(
                accelerometer: false, gyroscope: false, magnetometer: false, deviceMotion: false,
                altimeter: false)
        }

        public func start(sensor: MotionSensor, intervalMs: UInt32, sink: MotionSink) throws {
            throw NativeServiceError.Unavailable(reason: "Core Motion sensors exist only on iOS")
        }

        public func stop(sensor: MotionSensor) {}
    }

#endif
