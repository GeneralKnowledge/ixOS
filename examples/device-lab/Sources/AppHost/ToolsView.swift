@preconcurrency import ForgeFFI
import SwiftUI

/// The Tools tab: what a phone sitting needs (the checks, whether the test
/// servers are up, recent logs), then experiments and developer checks.
struct ToolsView: View {
    let model: LabModel
    @State private var transfers = TransferTestModel()
    @State private var sensorLab = SensorLabModel.live()
    @State private var aiLab = AiLabModel(
        lab: AiLab(model: ForgeAppleModel()), structured: AiLabStructured.run)
    @State private var llamaLab = LlamaLabModel()
    @State private var pythonLab = PythonLabModel.live()

    var body: some View {
        List {
            Section {
                NavigationLink {
                    PhoneChecksView()
                } label: {
                    Label("Checks", systemImage: "checklist")
                }
                .accessibilityIdentifier("tools.checks")
            } footer: {
                Text("The next phone sitting's checks: tick each one here and share the results.")
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
                    SensorLabView(model: sensorLab)
                } label: {
                    Label("Spatial Sensor Lab", systemImage: "move.3d")
                }
                .accessibilityIdentifier("tools.sensorLab")
            } footer: {
                Text("The phone in 3D as you turn it, and the magnetic field around it: strength, direction and change from a baseline.")
            }
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
                    LlamaLabView(model: llamaLab)
                } label: {
                    Label("Bonsai on iPhone", systemImage: "cpu")
                }
                .accessibilityIdentifier("tools.llamaLab")
            } footer: {
                Text("Download a ternary Bonsai model and run it on this phone with llama.cpp, in AI Lab.")
            }
            Section {
                NavigationLink {
                    PythonLabView(model: pythonLab)
                } label: {
                    Label("Python Lab", systemImage: "chevron.left.forwardslash.chevron.right")
                }
                .accessibilityIdentifier("tools.pythonLab")
            } footer: {
                Text("Write a Python script and run it on this phone with CPython.")
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
