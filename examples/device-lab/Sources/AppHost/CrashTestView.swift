import SwiftUI

/// Crashes the app on purpose, so a developer can check that Forge reads the
/// crash report down to the file and line (`forge device crashes --pull`,
/// docs/forge-device.md#crash-reports). Each button ends the app at once.
struct CrashTestView: View {
    let model: LabModel

    var body: some View {
        List {
            Section {
                Button(role: .destructive) {
                    model.crashTestSwift()
                } label: {
                    Label("Crash in Swift", systemImage: "swift")
                }
                .accessibilityIdentifier("crashTest.swift")
                .accessibilityHint("Closes Device Lab with a Swift runtime error")
            } footer: {
                Text("Force-unwraps a number that doesn't parse, in LabModel.swift.")
            }
            Section {
                Button(role: .destructive) {
                    model.crashTestRust()
                } label: {
                    Label("Crash in Rust", systemImage: "gearshape.2")
                }
                .accessibilityIdentifier("crashTest.rust")
                .accessibilityHint("Closes Device Lab with a Rust panic")
            } footer: {
                Text("Panics in a Rust callback that can't unwind, in crash_test.rs.")
            }
            Section {
                Text("Afterwards, on the computer: forge device crashes --app AppHostXtool --pull crashes/ --project examples/device-lab")
                    .font(.footnote.monospaced())
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
        }
        .groupedList()
        .navigationTitle("Crash Test")
        .inlineNavigationTitle()
    }
}
