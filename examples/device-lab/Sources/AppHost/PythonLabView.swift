import SwiftUI
import ForgeUI

/// Python Lab: write or pick a Python script and run it on the phone with
/// the embedded CPython.
struct PythonLabView: View {
    @Bindable var model: PythonLabModel

    var body: some View {
        List {
            Section {
                TextEditor(text: $model.code)
                    .font(.system(.footnote, design: .monospaced))
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                    .frame(minHeight: 180)
                    .accessibilityLabel("Python script")
                    .accessibilityIdentifier("pythonLab.code")
                Menu {
                    ForEach(model.samples) { sample in
                        Button(sample.title) { model.load(sample) }
                            .accessibilityIdentifier("pythonLab.sample.\(sample.id)")
                    }
                } label: {
                    Label("Examples", systemImage: "doc.text")
                }
                .accessibilityIdentifier("pythonLab.examples")
                if model.isRunning {
                    Button("Stop", systemImage: "stop.fill", role: .destructive) { model.stop() }
                        .accessibilityIdentifier("pythonLab.stop")
                } else {
                    Button("Run", systemImage: "play.fill") {
                        Task { await model.run() }
                    }
                    .disabled(!model.hasPython)
                    .accessibilityIdentifier("pythonLab.run")
                }
            } header: {
                Text("Script")
            } footer: {
                Text("Runs on this iPhone with CPython. If the last line is an expression, its value is shown. A script is stopped after \(model.timeLimit.components.seconds) s.")
            }
            Section {
                if model.isRunning {
                    ProgressView("Running…")
                        .accessibilityIdentifier("pythonLab.running")
                } else if model.succeeded != nil {
                    Text(model.output)
                        .font(.system(.footnote, design: .monospaced))
                        .textSelection(.enabled)
                        .accessibilityIdentifier("pythonLab.output")
                } else {
                    Text("Tap Run to see the output here.")
                        .foregroundStyle(.secondary)
                }
            } header: {
                Text("Output")
            } footer: {
                if let seconds = model.lastSeconds, !model.isRunning {
                    if model.succeeded == false {
                        Text("Ended with an error after \(Format.number(seconds, digits: 3)) s")
                            .forgeForeground(.danger)
                    } else {
                        Text("Finished in \(Format.number(seconds, digits: 3)) s")
                    }
                }
            }
            Section {
                if let version = model.version {
                    LabeledContent("Python", value: version)
                        .accessibilityIdentifier("pythonLab.version")
                }
                if let unavailable = model.unavailable {
                    Text(unavailable).font(.footnote).forgeForeground(.danger)
                        .accessibilityIdentifier("pythonLab.unavailable")
                }
            } header: {
                Text("Interpreter")
            }
        }
        .groupedList()
        .navigationTitle("Python Lab")
        .inlineNavigationTitle()
        .task { await model.prepare() }
    }
}
