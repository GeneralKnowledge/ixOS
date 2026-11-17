#if os(iOS)
@preconcurrency import RelayFFI
import ForgeAppKit
import SwiftUI
import UniformTypeIdentifiers

/// Screen 2: one paired device. Write a note or pick a file to send; what
/// went between the two devices is listed below.
struct PeerView: View {
    @Bindable var model: RelayModel
    let network: RelayNetwork
    let peer: Peer
    @Binding var preview: URL?

    @State private var note = ""
    @State private var sending = false
    @State private var pickingFile = false
    @FocusState private var writing: Bool

    var body: some View {
        List {
            Section {
                TextField("Write a note", text: $note, axis: .vertical)
                    .lineLimit(3...8)
                    .focused($writing)
                    .accessibilityIdentifier("peer.note")
                HStack {
                    Button {
                        sendNote()
                    } label: {
                        Label("Send Note", systemImage: "paperplane")
                    }
                    .disabled(sending || note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("peer.sendNote")
                    Spacer()
                    Button {
                        pickingFile = true
                    } label: {
                        Label("Send a File", systemImage: "doc.badge.plus")
                    }
                    .disabled(sending)
                    .accessibilityIdentifier("peer.sendFile")
                }
                .buttonStyle(.borderless)
            } footer: {
                Text(network.nearby.contains(peer.deviceId)
                     ? "\(peer.name) is nearby."
                     : "\(peer.name) isn't showing on this network. Open Relay on it, on the same Wi-Fi.")
            }
            let history = model.transfers(with: peer)
            if !history.isEmpty {
                Section("Between You") {
                    ForEach(history) { transfer in
                        TransferRow(transfer: transfer, model: model, preview: $preview)
                    }
                }
            }
        }
        .navigationTitle(peer.name)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Forget Device", systemImage: "trash", role: .destructive) {
                        model.askToForget(peer)
                    }
                    .accessibilityIdentifier("peer.forget")
                } label: {
                    Label("More", systemImage: "ellipsis.circle")
                }
                .accessibilityIdentifier("peer.more")
            }
        }
        .fileImporter(isPresented: $pickingFile, allowedContentTypes: [.item]) { result in
            switch result {
            case .success(let url): sendFile(url)
            case .failure(let error): model.report(error)
            }
        }
    }

    private func sendNote() {
        let text = note
        sending = true
        Task {
            let addresses = await network.addresses(for: peer.deviceId)
            if await model.send(note: text, to: peer, addresses: addresses) {
                note = ""
                writing = false
            }
            sending = false
        }
    }

    /// The picked file is copied first (ForgeAppKit's `ForgePickedFile`):
    /// the picker's access ends, and Rust reads the file until the transfer
    /// ends. The copy runs off the main thread (a large video takes seconds),
    /// keeps the file's name so the other device does too, and is deleted
    /// once the send has ended, sent or not.
    private func sendFile(_ picked: URL) {
        sending = true
        Task {
            do {
                let copy = try await ForgePickedFile.copy(of: picked)
                defer { copy.release() }
                let addresses = await network.addresses(for: peer.deviceId)
                await model.send(file: copy.url, to: peer, addresses: addresses)
            } catch {
                model.report(error)
            }
            sending = false
        }
    }
}
#endif
