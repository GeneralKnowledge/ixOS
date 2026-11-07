@preconcurrency import ForgeFFI
import SwiftUI

/// The Settings sheet: what is stored, and a network check that runs an
/// HTTPS request in Rust. The check is started with `.task(id:)`, so closing
/// the sheet cancels it, and the cancellation reaches the request in Rust.
struct SettingsView: View {
    @Bindable var model: NotesModel
    /// Changing it starts a check; nil means none was asked for.
    @State private var checkRequest: Int?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    LabeledContent("Notes", value: model.notes.count, format: .number)
                } header: {
                    Text("Storage")
                } footer: {
                    Text("Your notes are kept by the app's Rust code on this device and survive restarts.")
                }

                Section {
                    TextField("URL", text: $model.url)
                        .accessibilityIdentifier("settings.url")
                        .autocorrectionDisabled()
                        #if os(iOS)
                            .textContentType(.URL)
                            .textInputAutocapitalization(.never)
                            .keyboardType(.URL)
                        #endif
                        .submitLabel(.go)
                        .onSubmit(startCheck)
                    if model.isCheckingURL {
                        HStack {
                            ProgressView()
                            Text("Checking…").foregroundStyle(.secondary)
                            Spacer()
                            Button("Cancel") { model.cancelURLCheck() }
                                .accessibilityIdentifier("settings.cancelCheck")
                        }
                    } else {
                        Button("Check Connection", action: startCheck)
                            .accessibilityIdentifier("settings.check")
                    }
                    if let result = model.urlResult {
                        LabeledContent("Result", value: result)
                    }
                } header: {
                    Text("Network")
                } footer: {
                    Text("Sends a request from Rust. The server's certificate is checked by the system.")
                }
            }
            .navigationTitle("Settings")
            #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("settings.done")
                }
            }
            .task(id: checkRequest) {
                guard checkRequest != nil else { return }
                await model.checkURL()
            }
        }
    }

    private func startCheck() {
        checkRequest = (checkRequest ?? 0) + 1
    }
}
