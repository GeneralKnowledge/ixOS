@preconcurrency import ForgeFFI
import SwiftUI
import ForgeUI

/// Download a GGUF model (Ternary-Bonsai-2) and open AI Lab on it, running
/// on the phone through llama.cpp and Metal.
struct LlamaLabView: View {
    @Bindable var model: LlamaLabModel

    var body: some View {
        List {
            ForEach(model.files) { file in
                fileSection(file)
            }
            Section {
                LabeledContent("llama.cpp", value: model.backends)
                    .accessibilityIdentifier("llamaLab.backends")
                if let memory = model.availableMemory {
                    LabeledContent("Memory for this app", value: Format.memory(memory))
                }
                if let free = model.freeBytes {
                    LabeledContent("Storage for this app", value: Format.storage(UInt64(max(free, 0))))
                }
                if let load = model.lastLoadSeconds {
                    LabeledContent("Last model load", value: "\(Format.number(load, digits: 1)) s")
                }
            } header: {
                Text("This iPhone")
            } footer: {
                Text("The first answer after opening a model includes loading it, so its first-text time is longer. Keep Device Lab open while a model downloads.")
            }
        }
        .groupedList()
        .navigationTitle("Bonsai on iPhone")
        .inlineNavigationTitle()
        .task {
            while !Task.isCancelled {
                model.refresh()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    @ViewBuilder private func fileSection(_ file: LlamaModelFile) -> some View {
        Section {
            switch model.state(file) {
            case .missing:
                Button("Download", systemImage: "arrow.down.circle") { model.download(file) }
                    .accessibilityIdentifier("llamaLab.download.\(file.id)")
            case .downloading(let fraction):
                ProgressView(value: fraction) {
                    Text("Downloading \(Format.percent(fraction))")
                }
                .progressViewStyle(.forge)
                .accessibilityIdentifier("llamaLab.progress.\(file.id)")
                Button("Cancel Download", role: .cancel) { model.cancel(file) }
                    .accessibilityIdentifier("llamaLab.cancel.\(file.id)")
            case .failed(let reason):
                Text(reason).font(.footnote).forgeForeground(.danger)
                Button("Try Again", systemImage: "arrow.clockwise") { model.download(file) }
                    .accessibilityIdentifier("llamaLab.download.\(file.id)")
            case .ready:
                NavigationLink {
                    AiLabView(model: model.lab(for: file))
                } label: {
                    Label("Open in AI Lab", systemImage: "sparkles")
                }
                .accessibilityIdentifier("llamaLab.open.\(file.id)")
                Button("Remove Model", systemImage: "trash", role: .destructive) { model.remove(file) }
                    .accessibilityIdentifier("llamaLab.remove.\(file.id)")
            }
        } header: {
            Text(file.name)
        } footer: {
            Text(file.note)
        }
    }
}
