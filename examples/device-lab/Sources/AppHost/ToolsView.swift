@preconcurrency import ForgeFFI
import SwiftUI

/// The Tools tab: experiments and developer checks that aren't a sensor
/// reading or a hosted app.
struct ToolsView: View {
    let model: LabModel
    @State private var transfers = TransferTestModel()
    @State private var aiLab = AiLabModel(
        lab: AiLab(model: ForgeAppleModel()), structured: AiLabStructured.run)

    var body: some View {
        List {
            Section {
                NavigationLink {
                    AiLabView(model: aiLab)
                } label: {
                    Label("AI Lab", systemImage: "sparkles")
                }
                .accessibilityIdentifier("dashboard.aiLab")
            } footer: {
                Text("Experiments with Apple's on-device model: speed, Stop, structured output.")
            }
            Section {
                NavigationLink {
                    TransferTestView(model: transfers)
                } label: {
                    Label("Background Downloads", systemImage: "arrow.down.circle")
                }
                .accessibilityIdentifier("dashboard.transfers")
            } footer: {
                Text("Downloads that keep going after you leave the app.")
            }
            Section {
                NavigationLink {
                    ServersView()
                } label: {
                    Label("Servers", systemImage: "server.rack")
                }
                .accessibilityIdentifier("tools.servers")
            } footer: {
                Text("Whether the chat server and the take server answer from this phone.")
            }
            Section {
                NavigationLink {
                    LogsView()
                } label: {
                    Label("Logs", systemImage: "text.alignleft")
                }
                .accessibilityIdentifier("tools.logs")
            } footer: {
                Text("What Device Lab and its apps logged recently, to read or share without a cable.")
            }
            Section {
                NavigationLink {
                    CrashTestView(model: model)
                } label: {
                    Label("Crash Test", systemImage: "exclamationmark.triangle")
                }
                .accessibilityIdentifier("dashboard.crashTest")
            } footer: {
                Text("For developers: crash the app on purpose to check crash reports.")
            }
        }
        .groupedList()
        .navigationTitle("Tools")
    }
}
