@preconcurrency import ForgeFFI
import SwiftUI

/// A compass: Core Location's heading when location is allowed (true north
/// once a position is known), otherwise the fused device-motion heading
/// (magnetic north).
struct CompassView: View {
    let model: LabModel

    struct Reading: Equatable {
        var degrees: Double
        var isTrueNorth: Bool
        var accuracy: Double?
        var source: String
    }

    static func heading(_ s: LabSnapshot) -> Reading? {
        if let h = s.location.heading {
            if let t = h.trueDegrees {
                return Reading(degrees: t, isTrueNorth: true, accuracy: h.accuracyDegrees, source: "Location")
            }
            return Reading(
                degrees: h.magneticDegrees, isTrueNorth: false, accuracy: h.accuracyDegrees, source: "Location")
        }
        if let d = s.attitude.sample?.headingDegrees {
            return Reading(degrees: d, isTrueNorth: false, accuracy: nil, source: "Device motion")
        }
        return nil
    }

    var body: some View {
        let reading = Self.heading(model.snapshot)
        List {
            Section {
                Dial(degrees: reading?.degrees)
                    .frame(maxWidth: .infinity)
                    .frame(height: 280)
                    .listRowBackground(Color.clear)
            }
            Section {
                if let reading {
                    LabeledContent("Heading") {
                        Text("\(Format.cardinal(reading.degrees)) \(Format.degrees(reading.degrees))")
                            .monospacedDigit()
                    }
                    LabeledContent("North", value: reading.isTrueNorth ? "True" : "Magnetic")
                    if let accuracy = reading.accuracy {
                        LabeledContent("Accuracy", value: "± \(Format.degrees(accuracy))")
                    }
                    LabeledContent("Source", value: reading.source)
                } else {
                    Note("No heading yet. Move the phone in a figure 8 to calibrate the compass.")
                }
            } footer: {
                if !model.snapshot.permissions.location.allowsUse {
                    Text("Allow location for true north; without it the heading is magnetic.")
                }
            }
        }
        .groupedList()
        .navigationTitle("Compass")
        .inlineNavigationTitle()
    }
}

/// The compass rose. It turns so north on the dial points north.
private struct Dial: View {
    let degrees: Double?

    var body: some View {
        ZStack {
            Circle()
                .strokeBorder(.secondary.opacity(0.4), lineWidth: 1)
            ForEach(0..<72, id: \.self) { tick in
                Rectangle()
                    .fill(tick % 18 == 0 ? Color.primary : Color.secondary)
                    .frame(width: tick % 6 == 0 ? 2 : 1, height: tick % 6 == 0 ? 14 : 7)
                    .offset(y: -120)
                    .rotationEffect(.degrees(Double(tick) * 5))
            }
            ForEach(Array(["N", "E", "S", "W"].enumerated()), id: \.offset) { index, letter in
                Text(letter)
                    .font(.headline)
                    .foregroundStyle(letter == "N" ? Color.red : Color.primary)
                    .rotationEffect(.degrees(-Double(index) * 90 + (degrees ?? 0)))
                    .offset(y: -96)
                    .rotationEffect(.degrees(Double(index) * 90))
            }
        }
        .frame(width: 260, height: 260)
        .rotationEffect(.degrees(-(degrees ?? 0)))
        .overlay(alignment: .top) {
            // The lubber line: where the top of the phone points.
            Image(systemName: "arrowtriangle.down.fill")
                .foregroundStyle(.tint)
                .offset(y: -14)
        }
        .overlay {
            Text(degrees.map { Format.degrees($0) } ?? "–")
                .font(.system(size: 44, weight: .light, design: .rounded))
                .monospacedDigit()
        }
        .animation(.easeOut(duration: 0.2), value: degrees.map { ($0 / 2).rounded() })
        .accessibilityElement()
        .accessibilityLabel(degrees.map { "Heading \(Format.cardinal($0)), \(Int($0)) degrees" } ?? "No heading")
    }
}
