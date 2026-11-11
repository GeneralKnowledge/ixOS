@preconcurrency import FieldKitFFI
import ForgeAppKit
import SwiftUI

/// The Record sheet: name the trip, allow location if needed, start.
struct StartTripView: View {
    let model: TripsModel
    /// Called after the trip has started, to show the recording screen.
    var onStarted: () -> Void

    @State private var title = TripFormat.defaultTitle()
    @State private var starting = ForgeSubmission()
    @Environment(\.dismiss) private var dismiss

    private var trimmed: String { title.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        NavigationStack {
            Form {
                TextField("Name", text: $title)
                    .accessibilityIdentifier("start.title")
                Section {
                    permissionText
                }
            }
            .navigationTitle("New Trip")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .accessibilityIdentifier("start.cancel")
                }
                ToolbarItem(placement: .confirmationAction) {
                    if starting.isWorking {
                        ProgressView().accessibilityLabel("Starting")
                    } else {
                        Button("Start") { Task { await start() } }
                            .disabled(trimmed.isEmpty)
                            .accessibilityIdentifier("start.go")
                    }
                }
            }
            .forgeAlert(failure: $starting.failure)
        }
    }

    @ViewBuilder private var permissionText: some View {
        switch model.permission {
        case .denied, .restricted:
            Label(
                "Location is off for FieldKit, so the track won't record. Turn it on in Settings.",
                systemImage: "location.slash")
        case .notDetermined:
            Label("FieldKit asks for your location when you start.", systemImage: "location")
        default:
            Label("Your track is recorded while FieldKit is open.", systemImage: "location")
        }
    }

    private func start() async {
        let title = trimmed
        if await starting.run({ try await model.start(title: title) }) {
            dismiss()
            onStarted()
        }
    }
}
