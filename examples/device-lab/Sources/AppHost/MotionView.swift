@preconcurrency import ForgeFFI
import SwiftUI

/// Every motion sensor in full: each axis, the peaks, the measured rate.
struct MotionView: View {
    let model: LabModel

    private var s: LabSnapshot { model.snapshot }

    var body: some View {
        List {
            axisSection("Accelerometer", stats: s.accelerometer, unit: "g", color: .orange,
                footer: "Includes gravity: lying still it reads about 1 g.")
            axisSection("Gyroscope", stats: s.gyroscope, unit: "rad/s", color: .purple,
                footer: "How fast the phone turns around each axis.")
            axisSection("Magnetometer", stats: s.magnetometer, unit: "µT", digits: 0, color: .teal,
                footer: "Raw, including the phone's own magnetism. Earth's field is 25 to 65 µT.")
            attitudeSection
        }
        .groupedList()
        .navigationTitle("Motion")
        .inlineNavigationTitle()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Reset Peaks", systemImage: "arrow.counterclockwise") { model.reset() }
            }
        }
    }

    @ViewBuilder
    private func axisSection(
        _ title: String, stats: AxisStats, unit: String, digits: Int = 2, color: Color, footer: String
    ) -> some View {
        Section {
            if !stats.available {
                Note("This device has no \(title.lowercased()).")
            } else if let error = stats.error {
                Note(error)
            } else if let v = stats.latest {
                ReadingRow(
                    title: "Magnitude", symbol: "waveform.path.ecg",
                    value: "\(Format.number(stats.magnitude ?? 0, digits: digits)) \(unit)",
                    history: stats.history, color: color)
                axis("X", v.x, unit, digits)
                axis("Y", v.y, unit, digits)
                axis("Z", v.z, unit, digits)
                if let peak = stats.peakMagnitude {
                    LabeledContent("Peak", value: "\(Format.number(peak, digits: digits)) \(unit)")
                }
                LabeledContent("Rate", value: "\(Format.number(stats.rateHz, digits: 0)) Hz")
            } else {
                Note(stats.running ? "Starting…" : "Paused.")
            }
        } header: {
            Text(title)
        } footer: {
            Text(footer)
        }
    }

    private func axis(_ name: String, _ value: Double, _ unit: String, _ digits: Int) -> some View {
        LabeledContent(name) {
            Text("\(Format.signed(value, digits: digits)) \(unit)").monospacedDigit()
        }
    }

    @ViewBuilder private var attitudeSection: some View {
        let a = s.attitude
        Section {
            if !a.available {
                Note("Device motion is not available here.")
            } else if let error = a.error {
                Note(error)
            } else if let sample = a.sample {
                LabeledContent("Roll", value: Format.radiansAsDegrees(sample.attitude.roll))
                LabeledContent("Pitch", value: Format.radiansAsDegrees(sample.attitude.pitch))
                LabeledContent("Yaw", value: Format.radiansAsDegrees(sample.attitude.yaw))
                if let tilt = a.tiltDegrees {
                    LabeledContent("Tilt from Flat", value: Format.degrees(tilt))
                }
                ReadingRow(
                    title: "Movement", symbol: "figure.walk.motion",
                    value: "\(Format.number(sample.userAcceleration.magnitude, digits: 2)) g",
                    detail: a.peakMovement.map { "Peak \(Format.number($0)) g" },
                    history: a.movementHistory, color: .pink)
                LabeledContent("Gravity") {
                    Text(Format.vector(sample.gravity)).font(.footnote).monospacedDigit()
                }
                LabeledContent("Rotation") {
                    Text(Format.vector(sample.rotationRate)).font(.footnote).monospacedDigit()
                }
                if let field = sample.magneticField {
                    LabeledContent("Calibrated Field") {
                        Text(Format.vector(field, digits: 0)).font(.footnote).monospacedDigit()
                    }
                }
                LabeledContent("Compass Calibration", value: sample.magneticAccuracy.name)
                LabeledContent("Rate", value: "\(Format.number(a.rateHz, digits: 0)) Hz")
            } else {
                Note(a.running ? "Starting…" : "Paused.")
            }
        } header: {
            Text("Device Motion")
        } footer: {
            Text("The sensors fused: which way the phone faces, and how you move it with gravity taken out.")
        }
    }
}

extension Vector3 {
    var magnitude: Double { (x * x + y * y + z * z).squareRoot() }
}
