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
    @State private var loadFailed = false
    @State private var isAddingNote = false
    @State private var isRenaming = false
    @State private var newTitle = ""
    @State private var exported: ExportedFile?
    @State private var exporting = ForgeSubmission()
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        if let trip = model.trip(id: tripID) {
            List {
                Section {
                    TrackMap(track: track, notes: notes)
                        .frame(height: 280)
                        .listRowInsets(EdgeInsets())
                    HStack {
                        Stat(title: "Distance", value: TripFormat.distance(trip.distanceM))
                        Stat(title: "Time", value: TripFormat.elapsed(trip))
                        Stat(title: "Climb", value: TripFormat.distance(trip.climbM))
                    }
                }
                Section("Notes") {
                    if notes.isEmpty {
                        Text(loadFailed ? "The notes couldn't be read." : "No notes on this trip.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(notes) { note in
                        NoteRow(note: note)
                            .swipeActions {
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
            .sheet(item: $exported) { file in
                ShareSheet(file: file)
            }
            .alert("Rename Trip", isPresented: $isRenaming) {
                TextField("Name", text: $newTitle)
                    .accessibilityIdentifier("trip.renameField")
                Button("Cancel", role: .cancel) {}
                Button("Rename") { Task { try? await model.rename(trip, to: newTitle) } }
                    .accessibilityIdentifier("trip.renameSave")
            }
            .forgeAlert(failure: $exporting.failure)
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
                Button("Rename", systemImage: "pencil") {
                    newTitle = trip.title
                    isRenaming = true
                }
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
            loadFailed = false
        } catch {
            loadFailed = true
        }
    }

    private func export(_ format: ExportFormat) async {
        var url: URL?
        _ = await exporting.run { url = try await model.export(tripID, as: format) }
        if let url { exported = ExportedFile(url: url) }
    }
}

struct ExportedFile: Identifiable {
    let url: URL
    var id: URL { url }
}

/// The system share sheet for an exported file (AirDrop, Files, Mail...).
private struct ShareSheet: View {
    let file: ExportedFile
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 16) {
                Image(systemName: "doc")
                    .font(.largeTitle)
                    .accessibilityHidden(true)
                Text(file.url.lastPathComponent)
                    .font(.headline)
                ShareLink(item: file.url) {
                    Label("Share File", systemImage: "square.and.arrow.up")
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("export.share")
            }
            .padding()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("export.done")
                }
            }
        }
        .presentationDetents([.medium])
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
