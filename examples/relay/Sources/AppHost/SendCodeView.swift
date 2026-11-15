#if os(iOS)
@preconcurrency import RelayFFI
import ForgeUI
import SwiftUI
import UniformTypeIdentifiers

/// Send a file with a word code to anyone on this Wi-Fi (Relay M1). The
/// receiver types the code into Relay's "Receive with a Code", or runs
/// `relay-spike receive <code>` on a computer. Rust hashes and serves the
/// file and decides the code; this sheet picks the file, advertises the
/// code with Bonjour and shows where the send is.
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
                    Text("The receiver needs Relay, or relay-spike on a computer, on the same Wi-Fi.")
                }
                Section("Memory") {
                    MemoryReadout()
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
                switch result {
                case .success(let url): start(url)
                case .failure(let error): model.failSend(error.localizedDescription)
                }
            }
            // Rust's status twice a second while the send runs; the advert
            // follows the code.
            .task(id: model.isSendingCode) {
                while model.isSendingCode, (try? await Task.sleep(for: .milliseconds(500))) != nil {
                    model.refreshCodeSend()
                    network.advertiseCode(model.codeAdvert)
                }
                network.advertiseCode(model.codeAdvert)
            }
            .onAppear {
                model.resetCodeSend()
                network.watchCodeSenders { senders in
                    model.codeSendersNearby(senders)
                }
            }
            .onDisappear {
                network.stopWatchingCodeSenders()
                if !model.isSendingCode { network.advertiseCode(nil) }
            }
            .interactiveDismissDisabled(model.isSendingCode)
        }
    }

    @ViewBuilder private var status: some View {
        switch model.codeSend {
        case .idle:
            Text("Choose a file, and Relay shows a code for the receiver.")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("sendCode.status")
        case .preparing(let name, _):
            Label {
                Text("Getting \(name) ready…")
            } icon: {
                ProgressView()
            }
            .accessibilityIdentifier("sendCode.status")
        case .waiting(let code, _, _, _, let name, let size, let note):
            VStack(alignment: .leading, spacing: 8) {
                if let note {
                    Text(note)
                        .font(.callout)
                        .foregroundStyle(.orange)
                }
                Text(code)
                    .font(.title2.monospaced().weight(.semibold))
                    .textSelection(.enabled)
                    .accessibilityLabel("Code \(code.replacingOccurrences(of: "-", with: " "))")
                    .accessibilityIdentifier("sendCode.code")
                Text("\(name), \(RelayFormat.size(size)). Waiting for the receiver…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button("Copy Code", systemImage: "doc.on.doc") {
                    UIPasteboard.general.string = code
                }
                .accessibilityIdentifier("sendCode.copy")
            }
            .accessibilityIdentifier("sendCode.status")
        case .sending(_, _, _, _, let name, let size):
            Label {
                Text("Sending \(name), \(RelayFormat.size(size))")
            } icon: {
                ProgressView()
            }
            .accessibilityIdentifier("sendCode.status")
        case .sent(let name, let size, let seconds, let speed):
            VStack(alignment: .leading, spacing: 6) {
                Label("Sent \(name)", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                Text("\(RelayFormat.size(size)) in \(String(format: "%.1f", seconds)) s, \(String(format: "%.1f", speed)) MB/s")
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

    /// Copies the picked file into this app's temporary folder (the picker's
    /// access ends with this call; Rust reads the file for the whole send),
    /// then starts the send.
    private func start(_ url: URL) {
        model.preparing(name: url.lastPathComponent)
        Task {
            let copied = await Task.detached(priority: .userInitiated) { () -> Result<URL, Error> in
                let scoped = url.startAccessingSecurityScopedResource()
                defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                do {
                    let folder = FileManager.default.temporaryDirectory.appendingPathComponent("relay-send", isDirectory: true)
                    try? FileManager.default.removeItem(at: folder)
                    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                    let target = folder.appendingPathComponent(url.lastPathComponent)
                    try FileManager.default.copyItem(at: url, to: target)
                    return .success(target)
                } catch {
                    return .failure(error)
                }
            }.value
            switch copied {
            case .success(let file): model.send(fileAt: file.path)
            case .failure(let error): model.failSend("Couldn't read the file: \(error.localizedDescription)")
            }
        }
    }
}

/// This app's peak memory (the most it has used since launch) and what it
/// was when the sheet opened, so a transfer's cost shows. From `getrusage`:
/// resident memory, which counts shared pages too, so it is an upper bound.
struct MemoryReadout: View {
    @State private var atOpen: UInt64?
    @State private var peak: UInt64 = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Peak: \(RelayFormat.size(peak))")
                .font(.callout.monospacedDigit())
            if let atOpen {
                Text("Peak before this sheet: \(RelayFormat.size(atOpen))")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("memory.peak")
        .task {
            atOpen = Self.peakBytes()
            peak = Self.peakBytes()
            while (try? await Task.sleep(for: .milliseconds(500))) != nil {
                peak = Self.peakBytes()
            }
        }
    }

    /// `ru_maxrss` is in bytes on Apple platforms.
    static func peakBytes() -> UInt64 {
        var usage = rusage()
        guard getrusage(RUSAGE_SELF, &usage) == 0 else { return 0 }
        return UInt64(max(usage.ru_maxrss, 0))
    }
}
#endif
