#if os(iOS)
@preconcurrency import RelayFFI
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// Send a file with a word code (Relay M1): this iPhone waits on the Wi-Fi
/// and shows a code such as `37-grasp-gag-drum`; the other device types it
/// (Receive with a Code on another phone, or `relay-spike receive <code>`
/// on a computer). A wrong guess or a dropped transfer brings a new code.
struct SendCodeSheet: View {
    @Bindable var model: RelayModel
    let network: RelayNetwork

    @State private var picking = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    status
                } footer: {
                    Text("Both devices need to be on the same Wi-Fi. Keep Relay open until the file has gone.")
                }
                Section {
                    MemoryLine()
                }
            }
            .navigationTitle("Send with a Code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(model.isSendingCode ? "Stop" : "Done") {
                        if model.isSendingCode {
                            model.cancelCodeSend()
                        } else {
                            dismiss()
                        }
                    }
                    .accessibilityIdentifier("sendCode.close")
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Choose File") { picking = true }
                        .disabled(model.isSendingCode)
                        .accessibilityIdentifier("sendCode.choose")
                }
            }
            .fileImporter(isPresented: $picking, allowedContentTypes: [.item]) { result in
                if case .success(let url) = result { start(url) }
            }
            // The live code and progress, twice a second.
            .task(id: model.isSendingCode) {
                while model.isSendingCode, (try? await Task.sleep(for: .milliseconds(500))) != nil {
                    if !model.refreshCodeSend(advertise: advertise) {
                        network.stopCodeAdvert()
                    }
                }
            }
            .onAppear { model.resetCodeSend() }
            .onDisappear {
                if !model.isSendingCode { network.stopCodeAdvert() }
            }
            .interactiveDismissDisabled(model.isSendingCode)
        }
    }

    @ViewBuilder private var status: some View {
        switch model.codeSend {
        case .idle:
            Text("Choose a file, then give the other device the code.")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("sendCode.status")
        case .starting:
            Label {
                Text("Getting the file ready…")
            } icon: {
                ProgressView()
            }
            .accessibilityIdentifier("sendCode.status")
        case .running(let status, let since):
            VStack(alignment: .leading, spacing: 10) {
                Text(status.code)
                    .font(.system(.title, design: .monospaced).weight(.semibold))
                    .textSelection(.enabled)
                    .accessibilityLabel("Code \(status.code)")
                    .accessibilityIdentifier("sendCode.code")
                Text("\(status.name), \(RelayFormat.size(status.size))")
                    .font(.subheadline)
                if !status.note.isEmpty {
                    Text(status.note)
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
                Label {
                    Text(status.state == .sending ? "Sending…" : "Waiting for the other device…")
                } icon: {
                    ProgressView()
                }
                TimelineView(.periodic(from: since, by: 1)) { context in
                    Text("\(Int(context.date.timeIntervalSince(since))) s")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("sendCode.status")
        case .sent(let status):
            VStack(alignment: .leading, spacing: 6) {
                Label("Sent \(status.name)", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                Text(Self.summary(status))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("sendCode.status")
        case .failed(let reason):
            Label(reason, systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
                .accessibilityIdentifier("sendCode.status")
        }
    }

    private func advertise(_ number: UInt8, _ endpointID: String, _ port: UInt16) {
        network.advertiseCodeSender(number: number, endpointID: endpointID, port: port)
    }

    /// The picked file is copied into Relay's caches first: the picker's
    /// access ends, and Rust reads the file until the send ends.
    private func start(_ picked: URL) {
        let copy: URL
        do {
            copy = try Self.outboxCopy(of: picked)
        } catch {
            model.report(error)
            return
        }
        Task {
            let inUse = await network.codeNumbersInUse()
            await model.send(file: copy, numbersInUse: inUse, advertise: advertise)
        }
    }

    private static func outboxCopy(of picked: URL) throws -> URL {
        let access = picked.startAccessingSecurityScopedResource()
        defer { if access { picked.stopAccessingSecurityScopedResource() } }
        let folder = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("RelayCodeOutbox", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        // Same name, so the other device keeps it.
        let copy = folder.appendingPathComponent(picked.lastPathComponent)
        try FileManager.default.copyItem(at: picked, to: copy)
        return copy
    }

    /// "200 MB in 12.6 s, 15.9 MB/s".
    static func summary(_ status: CodeSendStatus) -> String {
        "\(RelayFormat.size(status.size)) in \(String(format: "%.1f", status.seconds)) s, "
            + "\(String(format: "%.1f", status.megabytesPerSecond)) MB/s"
    }
}
#endif
