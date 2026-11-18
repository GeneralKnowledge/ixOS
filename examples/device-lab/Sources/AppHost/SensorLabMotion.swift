import Foundation

#if os(iOS)
    import CoreMotion

    /// Spatial Sensor Lab's samples from Core Motion: one `CMMotionManager`,
    /// one device-motion stream referenced to magnetic north, delivered on
    /// the main queue while the screen is open.
    ///
    /// The This iPhone tab's sensors (the framework motion service) stop
    /// when another tab is on screen, so this is the only stream running
    /// while the lab is open.
    @MainActor
    final class CoreMotionSensorLabSource: SensorLabSource {
        private let manager = CMMotionManager()

        var availability: SensorLabAvailability {
            SensorLabAvailability(
                deviceMotion: manager.isDeviceMotionAvailable,
                magneticNorth: CMMotionManager.availableAttitudeReferenceFrames()
                    .contains(.xMagneticNorthZVertical))
        }

        func start(
            onSample: @escaping @Sendable @MainActor (SensorLabSample) -> Void,
            failed: @escaping @Sendable @MainActor (String) -> Void
        ) {
            let magnetic = availability.magneticNorth
            manager.deviceMotionUpdateInterval = SensorLabModel.interval
            manager.startDeviceMotionUpdates(
                using: magnetic ? .xMagneticNorthZVertical : .xArbitraryCorrectedZVertical,
                to: .main
            ) { motion, error in
                // The main queue runs on the main thread.
                MainActor.assumeIsolated {
                    if let motion {
                        onSample(Self.sample(motion, magnetic: magnetic))
                    } else if let error {
                        failed(error.localizedDescription)
                    }
                }
            }
        }

        func stop() {
            manager.stopDeviceMotionUpdates()
        }

        private static func sample(_ m: CMDeviceMotion, magnetic: Bool) -> SensorLabSample {
            let r = m.attitude.rotationMatrix
            let calibration: FieldCalibration =
                switch m.magneticField.accuracy {
                case .low: .low
                case .medium: .medium
                case .high: .high
                default: .uncalibrated
                }
            let f = m.magneticField.field
            return SensorLabSample(
                timestamp: m.timestamp,
                rotation: SensorRotation(
                    m11: r.m11, m12: r.m12, m13: r.m13,
                    m21: r.m21, m22: r.m22, m23: r.m23,
                    m31: r.m31, m32: r.m32, m33: r.m33),
                roll: m.attitude.roll,
                pitch: m.attitude.pitch,
                yaw: m.attitude.yaw,
                gravity: SensorVector(m.gravity.x, m.gravity.y, m.gravity.z),
                userAcceleration: SensorVector(
                    m.userAcceleration.x, m.userAcceleration.y, m.userAcceleration.z),
                field: magnetic ? SensorVector(f.x, f.y, f.z) : nil,
                calibration: magnetic ? calibration : .uncalibrated)
        }
    }
#endif

extension SensorLabModel {
    /// The model on Core Motion; on a Mac host (model tests) a source with
    /// no sensors.
    static func live() -> SensorLabModel {
        #if os(iOS)
            SensorLabModel(source: CoreMotionSensorLabSource())
        #else
            SensorLabModel(source: NoSensorLabSource())
        #endif
    }
}

/// A source with no sensors: the Mac host, where Core Motion has no device
/// motion.
@MainActor
final class NoSensorLabSource: SensorLabSource {
    var availability: SensorLabAvailability {
        SensorLabAvailability(deviceMotion: false, magneticNorth: false)
    }

    func start(
        onSample: @escaping @Sendable @MainActor (SensorLabSample) -> Void,
        failed: @escaping @Sendable @MainActor (String) -> Void
    ) {}

    func stop() {}
}
