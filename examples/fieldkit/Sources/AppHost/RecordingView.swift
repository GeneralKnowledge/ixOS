@preconcurrency import FieldKitFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

private let log = ForgeLog("Recording")

/// The trip being recorded: the track so far on a map, the live totals,
/// Add Note and Stop.
struct RecordingView: View {
    @Bindable var model: TripsModel
    @State private var track: [TrackPoint] = []
    @State private var notes: [Note] = []
    @State private var isAddingNote = false
    /// Map reads that failed in a row; from the third, the screen says so.
    @State private var failedReads = 0
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        if let trip = model.recording {
            VStack(spacing: 0) {
                TrackMap(track: track, notes: notes, follow: true)
                    .frame(maxHeight: .infinity)
                stats(trip)
            }
            .navigationTitle(trip.title)
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            // A new fix or note moves these: read the track again.
            .task(id: "\(trip.pointCount)-\(trip.noteCount)") { await load(trip.id) }
            .sheet(isPresented: $isAddingNote) {
                AddNoteView(model: model, tripID: trip.id)
            }
        } else {
            // Stopped (here or elsewhere): nothing to show.
            Color.clear.onAppear { dismiss() }
        }
    }

    private func stats(_ trip: TripSummary) -> some View {
        VStack(spacing: 12) {
            if failedReads >= 3 {
                Label("The map can't be updated right now. Recording carries on.", systemImage: "map")
                    .font(.footnote)
                    .forgeForeground(.warning)
                    .accessibilityIdentifier("recording.mapProblem")
            }
            if let problem = model.locationProblem {
                HStack {
                    Label(problem, systemImage: "location.slash")
                        .font(.footnote)
                    if !model.permission.allowsUse {
                        Button("Allow") { Task { await model.requestPermission() } }
                            .accessibilityIdentifier("recording.allowLocation")
                    }
                }
                .forgeForeground(.warning)
            }
            TimelineView(.periodic(from: .now, by: 1)) { context in
                HStack {
                    Stat(title: "Distance", value: TripFormat.distance(trip.distanceM))
                    Stat(title: "Time", value: TripFormat.elapsed(trip, now: context.date))
                    Stat(title: "Climb", value: TripFormat.distance(trip.climbM))
                }
            }
            HStack {
                Button("Add Note", systemImage: "note.text.badge.plus") { isAddingNote = true }
                    .buttonStyle(.bordered)
                    .accessibilityIdentifier("recording.addNote")
                Spacer()
                Button("Stop", systemImage: "stop.fill", role: .destructive) { model.askToStop() }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("recording.stop")
            }
            .controlSize(.large)
        }
        .padding()
        .background(.bar)
    }

    private func load(_ tripID: Int64) async {
        do {
            let loaded = try await model.load(tripID)
            track = loaded.track
            notes = loaded.detail.notes
            failedReads = 0
        } catch is CancellationError {
            // A newer fix started the next read.
        } catch {
            // The totals stay on screen and the next fix reads again; after
            // three failures in a row the screen says the map is stuck.
            failedReads += 1
            log.warn("Couldn't read the recording's map data (\(failedReads) in a row): \(TripsModel.describe(error))")
        }
    }
}

/// One number with its name under it.
struct Stat: View {
    let title: String
    let value: String

    var body: some View {
        VStack {
            Text(value)
                .font(.title3.monospacedDigit().bold())
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .combine)
    }
}
