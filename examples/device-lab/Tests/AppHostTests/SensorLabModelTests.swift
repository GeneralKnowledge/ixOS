import AppHost
import Foundation
import XCTest

/// Spatial Sensor Lab's rules: starting and stopping, the baseline, the
/// anomaly and its history, and the 3D view's geometry.
@MainActor
final class SensorLabModelTests: XCTestCase {
    @MainActor
    private final class ScriptedSource: SensorLabSource {
        var availability = SensorLabAvailability(deviceMotion: true, magneticNorth: true)
        var running = false
        var onSample: (@Sendable @MainActor (SensorLabSample) -> Void)?
        var failed: (@Sendable @MainActor (String) -> Void)?

        func start(
            onSample: @escaping @Sendable @MainActor (SensorLabSample) -> Void,
            failed: @escaping @Sendable @MainActor (String) -> Void
        ) {
            running = true
            self.onSample = onSample
            self.failed = failed
        }

        func stop() { running = false }

        func send(_ sample: SensorLabSample) { onSample?(sample) }
    }

    private var time = 0.0

    private func sample(
        field: SensorVector?, calibration: FieldCalibration = .high, rotation: SensorRotation = .identity
    ) -> SensorLabSample {
        time += 1.0 / 60
        return SensorLabSample(
            timestamp: time, rotation: rotation, roll: 0, pitch: 0, yaw: 0,
            gravity: SensorVector(0, 0, -1), userAcceleration: .zero, field: field, calibration: calibration)
    }

    /// Earth's field somewhere in Britain: about 49 µT, dipping 67°.
    private let earth = SensorVector(19, 0, -45)

    func testStartsAndStopsTheSource() {
        let source = ScriptedSource()
        let model = SensorLabModel(source: source)
        model.start()
        XCTAssertTrue(source.running)
        XCTAssertEqual(model.phase, .running)
        model.stop()
        XCTAssertFalse(source.running)
        XCTAssertEqual(model.phase, .stopped)
    }

    func testNoMotionSensorsIsUnavailable() {
        let source = ScriptedSource()
        source.availability = SensorLabAvailability(deviceMotion: false, magneticNorth: false)
        let model = SensorLabModel(source: source)
        model.start()
        XCTAssertFalse(source.running)
        guard case .unavailable = model.phase else { return XCTFail("expected unavailable") }
    }

    func testAFailureStopsTheSource() {
        let source = ScriptedSource()
        let model = SensorLabModel(source: source)
        model.start()
        source.failed?("Motion is busy")
        XCTAssertFalse(source.running)
        XCTAssertEqual(model.phase, .failed("Motion is busy"))
    }

    func testBaselineWaitsForCalibration() {
        let source = ScriptedSource()
        let model = SensorLabModel(source: source)
        model.start()
        source.send(sample(field: earth, calibration: .uncalibrated))
        XCTAssertFalse(model.fieldReady)
        model.setBaseline()
        XCTAssertNil(model.collecting)
    }

    func testBaselineAveragesThenAnomalyMeasuresTheChange() {
        let source = ScriptedSource()
        let model = SensorLabModel(source: source)
        model.start()
        source.send(sample(field: earth))
        model.setBaseline()
        for i in 0..<SensorLabModel.baselineSamples {
            // Noise of ±1 µT averages out.
            let noise = i.isMultiple(of: 2) ? 1.0 : -1.0
            source.send(sample(field: earth + SensorVector(noise, 0, 0)))
        }
        XCTAssertNil(model.collecting)
        let baseline = try! XCTUnwrap(model.baseline)
        XCTAssertEqual(baseline.field.x, earth.x, accuracy: 1e-9)
        XCTAssertEqual(baseline.strength, earth.length, accuracy: 1e-9)

        // A magnet adds 30 µT along the phone's x axis.
        source.send(sample(field: earth + SensorVector(30, 0, 0)))
        XCTAssertEqual(model.anomaly ?? 0, 30, accuracy: 1e-9)
        XCTAssertGreaterThan(model.strengthChange ?? 0, 0)
        XCTAssertEqual(model.peakAnomaly ?? 0, 30, accuracy: 1e-9)
        XCTAssertEqual(SensorLabModel.describe(anomaly: 30), "Clear disturbance")
    }

    func testTurningThePhoneIsNotAnAnomaly() {
        let source = ScriptedSource()
        let model = SensorLabModel(source: source)
        model.start()
        source.send(sample(field: earth))
        model.setBaseline()
        for _ in 0..<SensorLabModel.baselineSamples { source.send(sample(field: earth)) }

        // Upside down (turned half a turn about the phone's y axis): the
        // same world field reads differently in the phone's axes.
        let flipped = SensorRotation(m11: -1, m12: 0, m13: 0, m21: 0, m22: 1, m23: 0, m31: 0, m32: 0, m33: -1)
        source.send(sample(field: flipped.toDevice(earth), rotation: flipped))
        XCTAssertEqual(model.anomaly ?? -1, 0, accuracy: 1e-9)
    }

    func testHistoryIsTenASecondAndCapped() {
        let source = ScriptedSource()
        let model = SensorLabModel(source: source)
        model.start()
        source.send(sample(field: earth))
        model.setBaseline()
        for _ in 0..<SensorLabModel.baselineSamples { source.send(sample(field: earth)) }
        XCTAssertTrue(model.history.isEmpty)

        for _ in 0..<60 { source.send(sample(field: earth)) }
        XCTAssertEqual(model.history.count, 10)
        XCTAssertEqual(model.rateHz, 60, accuracy: 0.5)

        for _ in 0..<(SensorLabModel.historyLength * SensorLabModel.historyStride) {
            source.send(sample(field: earth))
        }
        XCTAssertEqual(model.history.count, SensorLabModel.historyLength)

        model.clearBaseline()
        XCTAssertNil(model.baseline)
        XCTAssertTrue(model.history.isEmpty)
        XCTAssertNil(model.anomaly)
    }

    func testInclinationOfEarthsField() {
        let source = ScriptedSource()
        let model = SensorLabModel(source: source)
        model.start()
        source.send(sample(field: earth))
        XCTAssertEqual(model.inclinationDegrees ?? 0, atan2(45, 19) * 180 / .pi, accuracy: 1e-9)
    }

    func testRotationRoundTrips() {
        // A quarter turn about the vertical.
        let r = SensorRotation(m11: 0, m12: 1, m13: 0, m21: -1, m22: 0, m23: 0, m31: 0, m32: 0, m33: 1)
        let v = SensorVector(1, 2, 3)
        let back = r.toWorld(r.toDevice(v))
        XCTAssertEqual(back, v)
    }

    func testSceneShowsTheScreenOfAFlatPhoneAndTheBackOfAFlippedOne() {
        let scene = SensorLabScene()
        let flat = scene.phoneFaces(.identity).map(\.kind)
        XCTAssertTrue(flat.contains(.screen))
        XCTAssertFalse(flat.contains(.back))

        let flipped = SensorRotation(m11: -1, m12: 0, m13: 0, m21: 0, m22: 1, m23: 0, m31: 0, m32: 0, m33: -1)
        let upsideDown = scene.phoneFaces(flipped).map(\.kind)
        XCTAssertTrue(upsideDown.contains(.back))
        XCTAssertFalse(upsideDown.contains(.screen))
    }

    func testSceneDrawsNorthAwayFromTheViewer() {
        let scene = SensorLabScene()
        let north = scene.project(SensorVector(1, 0, 0))
        let south = scene.project(SensorVector(-1, 0, 0))
        // Further away, so higher on screen (smaller y) and behind.
        XCTAssertLessThan(north.y, south.y)
        XCTAssertGreaterThan(north.depth, 0)
        // East is to the right.
        XCTAssertGreaterThan(scene.project(SensorVector(0, -1, 0)).x, 0)
    }
}
