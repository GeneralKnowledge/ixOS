@preconcurrency import FieldKitFFI
import ForgeAppKit
import ForgeUI
import SwiftUI

/// A finished trip: its map, totals and notes, with Share (GPX or GeoJSON),
/// Rename and Delete.
struct TripDetailView: View {
    @Bindable var model: TripsModel
    let tripID: Int64

    @State private var track: [TrackPoint] = []
    @State private var notes: [Note] = []
    @State private var profile = Profile(elevation: [], splits: [])
    @State private var loadFailed = false
    @State private var isAddingNote = false
    @State private var isRenaming = false
    @State private var exported: URL?
    /// Share and Rename: a failure is shown, never dropped.
    @State private var acting = ForgeSubmission()
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        if let trip = model.trip(id: tripID) {
            List {
                Section {
                    TrackMap(track: track, notes: notes)
                        .frame(height: 280)
                        .listRowInsets(EdgeInsets())
                    HStack {
                        ForgeStat("Distance", value: TripFormat.distance(trip.distanceM))
                        ForgeStat("Time", value: TripFormat.elapsed(trip))
                        ForgeStat("Climb", value: TripFormat.distance(trip.climbM))
                    }
                }
                Section("Elevation") {
                    ElevationChart(points: profile.elevation)
                }
                Section("Pace") {
                    PaceChart(splits: profile.splits)
                }
                Section("Notes") {
                    if notes.isEmpty {
                        Text(loadFailed ? "The notes couldn't be read." : "No notes on this trip.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(notes) { note in
                        NoteRow(note: note)
                            .forgeRowActions {
                                Button("Delete", systemImage: "trash", role: .destructive) {
                                    Task { await model.removeNote(note) }
                                }
                                .accessibilityIdentifier("trip.deleteNote")
                            }
                    }
                }
            }
            .navigationTitle(trip.title)
            .task(id: "\(trip.pointCount)-\(trip.noteCount)") { await load() }
            .toolbar { toolbar(trip) }
            .sheet(isPresented: $isAddingNote) {
                AddNoteView(model: model, tripID: tripID)
            }
            .forgeShareFile($exported)
            .forgeRename("Rename Trip", isPresented: $isRenaming, name: trip.title, id: "renameTrip") { title in
                try await model.rename(trip, to: title)
            }
            .forgeAlert(failure: $acting.failure)
        } else {
            Color.clear.onAppear { dismiss() }
        }
    }

    @ToolbarContentBuilder
    private func toolbar(_ trip: TripSummary) -> some ToolbarContent {
        ToolbarItem(placement: .primaryAction) {
            Menu("Actions", systemImage: "ellipsis.circle") {
                Button("Add Note", systemImage: "note.text.badge.plus") { isAddingNote = true }
                    .accessibilityIdentifier("trip.addNote")
                Button("Share as GPX", systemImage: "square.and.arrow.up") {
                    Task { await export(.gpx) }
                }
                .accessibilityIdentifier("trip.shareGPX")
                Button("Share as GeoJSON", systemImage: "square.and.arrow.up") {
                    Task { await export(.geoJson) }
                }
                .accessibilityIdentifier("trip.shareGeoJSON")
                Button("Rename", systemImage: "pencil") { isRenaming = true }
                .accessibilityIdentifier("trip.rename")
                Button("Delete Trip", systemImage: "trash", role: .destructive) {
                    model.askToDelete(trip)
                }
                .accessibilityIdentifier("trip.delete")
            }
            .accessibilityIdentifier("trip.actions")
        }
    }

    private func load() async {
        do {
            let loaded = try await model.load(tripID)
            track = loaded.track
            notes = loaded.detail.notes
            profile = loaded.profile
            loadFailed = false
        } catch {
            loadFailed = true
        }
    }

    private func export(_ format: ExportFormat) async {
        var url: URL?
        _ = await acting.run { url = try await model.export(tripID, as: format) }
        if let url { exported = url }
    }
}

private struct NoteRow: View {
    let note: Note

    var body: some View {
        HStack(alignment: .top) {
            Image(systemName: note.symbol)
                .foregroundStyle(.tint)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(note.mapTitle)
                Text(TripFormat.date(note.createdMs), format: .dateTime.hour().minute())
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("trip.note")
    }
}
