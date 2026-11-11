@preconcurrency import FieldKitFFI
import ForgeAppKit
import SwiftUI

/// The trip being recorded: the track so far on a map, the live totals,
/// Add Note and Stop.
struct RecordingView: View {
    @Bindable var model: TripsModel
    @State private var track: [TrackPoint] = []
    @State private var notes: [Note] = []
    @State private var isAddingNote = false
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
            if let problem = model.locationProblem {
                HStack {
                    Label(problem, systemImage: "location.slash")
                        .font(.footnote)
                    if !model.permission.allowsUse {
                        Button("Allow") { Task { await model.requestPermission() } }
                            .accessibilityIdentifier("recording.allowLocation")
                    }
                }
                .foregroundStyle(.orange)
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
        } catch {
            // The list shows the totals; a failed read of the map data is
            // retried on the next fix.
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
