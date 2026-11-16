#if os(iOS)
@preconcurrency import RelayFFI
import Darwin
import ForgeAppKit
import Foundation
import ForgeUI
import SwiftUI
import UIKit
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
    /// The copy of the file being sent, kept until the next send starts.
    @State private var outgoing: ForgePickedFile?
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase

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
                let model = model
                network.watchCodeSenders { senders in
                    model.codeSendersNearby(senders)
                }
            }
            .onChange(of: scenePhase) {
                if scenePhase == .active { network.readvertiseCode(model.codeAdvert) }
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
                        .forgeForeground(.warning)
                }
                Text(code)
                    .font(.title2.monospaced().weight(.semibold))
                    .textSelection(.enabled)
                    .accessibilityLabel("Code \(code.replacingOccurrences(of: "-", with: " "))")
                    .accessibilityIdentifier("sendCode.code")
                Text("\(name), \(RelayFormat.size(size)). Waiting for the receiver… Keep Relay open: others can only find this code while it's on screen.")
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

    /// Copies the picked file off the main thread (ForgeAppKit's
    /// `ForgePickedFile`: the picker's access ends with this call, and Rust
    /// reads the file for the whole send), then starts the send. The
    /// previous send's copy is deleted when the next one starts.
    private func start(_ url: URL) {
        model.preparing(name: url.lastPathComponent)
        Task {
            outgoing?.release()
            outgoing = nil
            do {
                let copy = try await ForgePickedFile.copy(of: url)
                outgoing = copy
                model.send(fileAt: copy.url.path)
            } catch {
                model.failSend("Couldn't read the file: \(error.localizedDescription)")
            }
        }
    }
}

/// This app's memory footprint now and its peak since the sheet opened:
/// `phys_footprint`, the number iOS holds against the app's memory limit.
struct MemoryReadout: View {
    @State private var now: UInt64 = 0
    @State private var peak: UInt64 = 0

    var body: some View {
        Text("Memory \(RelayFormat.size(now)), peak \(RelayFormat.size(peak))")
            .font(.callout.monospacedDigit())
            .accessibilityIdentifier("memory.peak")
            .task {
                while !Task.isCancelled {
                    if let footprint = Self.footprint() {
                        now = footprint
                        peak = max(peak, footprint)
                    }
                    try? await Task.sleep(for: .milliseconds(500))
                }
            }
    }

    static func footprint() -> UInt64? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(
            MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size
        )
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? info.phys_footprint : nil
    }
}
#endif
