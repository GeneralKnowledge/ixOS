import Charts
@preconcurrency import FieldKitFFI
import SwiftUI

/// Altitude over distance, from FieldKit's Rust core (`track::profile`):
/// it is already thinned to a few hundred points, so the chart draws what
/// it is given.
struct ElevationChart: View {
    let points: [ElevationPoint]

    var body: some View {
        if points.count < 2 {
            Text("No altitude was recorded on this trip.")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("trip.elevationChart")
        } else {
            Chart(points, id: \.distanceM) { point in
                AreaMark(
                    x: .value("Distance (km)", point.distanceM / 1000),
                    yStart: .value("Lowest", lowest),
                    yEnd: .value("Altitude (m)", point.altitudeM)
                )
                .foregroundStyle(.tint.opacity(0.2))
                LineMark(
                    x: .value("Distance (km)", point.distanceM / 1000),
                    y: .value("Altitude (m)", point.altitudeM)
                )
                .foregroundStyle(.tint)
            }
            .chartYScale(domain: lowest ... highest)
            .chartXAxisLabel("km")
            .chartYAxisLabel("m")
            .frame(height: 160)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Elevation")
            .accessibilityValue(
                "From \(Int(lowest.rounded())) to \(Int(highest.rounded())) metres"
            )
            .accessibilityIdentifier("trip.elevationChart")
        }
    }

    private var lowest: Double { points.map(\.altitudeM).min() ?? 0 }
    private var highest: Double {
        // A flat trip still gets a visible band.
        max(points.map(\.altitudeM).max() ?? 0, lowest + 10)
    }
}

/// The time for each kilometre; a short last split is drawn at the same
/// rate (seconds per kilometre) so it compares with the rest.
struct PaceChart: View {
    let splits: [PaceSplit]

    var body: some View {
        if splits.isEmpty {
            Text("Pace shows once a trip has covered 100 m.")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("trip.paceChart")
        } else {
            Chart(splits, id: \.number) { split in
                BarMark(
                    x: .value("Kilometre", Int(split.number)),
                    y: .value("Minutes per km", split.secondsPerKm / 60)
                )
                .foregroundStyle(.tint)
                .accessibilityLabel(label(split))
                .accessibilityValue(TripFormat.pace(secondsPerKm: split.secondsPerKm))
            }
            .chartXAxisLabel("km")
            .chartYAxisLabel("min/km")
            .frame(height: 160)
            .accessibilityIdentifier("trip.paceChart")
        }
    }

    /// "Km 3", or "Last 420 m" for a short final split.
    private func label(_ split: PaceSplit) -> String {
        split.distanceM < 999
            ? "Last \(Int(split.distanceM.rounded())) m"
            : "Km \(split.number)"
    }
}
