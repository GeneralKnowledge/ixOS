@preconcurrency import FieldKitFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// Screen 1: the trip being recorded at the top, then every trip, newest
/// first. Record starts a new trip; tapping a trip opens it.
struct TripListView: View {
    @Bindable var model: TripsModel
    @State private var isStarting = false
    @State private var path: [Route] = []

    enum Route: Hashable {
        case recording
        case trip(Int64)
    }

    var body: some View {
        NavigationStack(path: $path) {
            List {
                if let trip = model.recording {
                    Section {
                        NavigationLink(value: Route.recording) {
                            RecordingRow(trip: trip, problem: model.locationProblem)
                        }
                        .accessibilityIdentifier("trips.recording")
                    }
                }
                Section {
                    ForEach(model.trips.filter { !$0.recording }) { trip in
                        NavigationLink(value: Route.trip(trip.id)) {
                            TripRow(trip: trip)
                        }
                        .accessibilityIdentifier("trips.row")
                        .swipeActions {
                            Button("Delete", systemImage: "trash", role: .destructive) {
                                model.askToDelete(trip)
                            }
                            .accessibilityIdentifier("trips.delete")
                        }
                    }
                }
            }
            .overlay { emptyState }
            .navigationTitle("FieldKit")
            .navigationDestination(for: Route.self) { route in
                switch route {
                case .recording: RecordingView(model: model)
                case .trip(let id): TripDetailView(model: model, tripID: id)
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    if model.recording == nil {
                        Button("Record", systemImage: "record.circle") { isStarting = true }
                            .accessibilityIdentifier("trips.record")
                    }
                }
            }
            .sheet(isPresented: $isStarting) {
                StartTripView(model: model) { path = [.recording] }
            }
            .forgeAlert($model.alert) { action in Task { await model.handle(action) } }
            .sensoryFeedback(.success, trigger: model.saveCount)
        }
    }

    @ViewBuilder private var emptyState: some View {
        if model.trips.isEmpty {
            ForgeStateView.empty(
                "No Trips", systemImage: "map",
                description: "Record a walk, ride or site visit. Your track and notes stay on this phone.",
                action: ForgeStateAction("Record a Trip", identifier: "trips.recordFirst") { isStarting = true }
            )
        }
    }
}

private struct TripRow: View {
    let trip: TripSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(trip.title)
                .font(.headline)
            HStack(spacing: 12) {
                Text(TripFormat.date(trip.startedMs), format: .dateTime.day().month().year())
                Text(TripFormat.distance(trip.distanceM))
                Text(TripFormat.elapsed(trip))
                if trip.noteCount > 0 {
                    Label("\(trip.noteCount)", systemImage: "note.text")
                        .accessibilityLabel("\(trip.noteCount) notes")
                }
            }
            .font(.subheadline)
            .foregroundStyle(.secondary)
        }
        .accessibilityElement(children: .combine)
    }
}

private struct RecordingRow: View {
    let trip: TripSummary
    let problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label("Recording", systemImage: "record.circle")
                .font(.caption.bold())
                .foregroundStyle(.red)
            Text(trip.title)
                .font(.headline)
            TimelineView(.periodic(from: .now, by: 1)) { context in
                Text("\(TripFormat.distance(trip.distanceM)) · \(TripFormat.elapsed(trip, now: context.date))")
                    .font(.subheadline.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            if let problem {
                Label(problem, systemImage: "location.slash")
                    .font(.footnote)
                    .foregroundStyle(.orange)
            }
        }
        .accessibilityElement(children: .combine)
    }
}
