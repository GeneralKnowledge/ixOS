#if os(iOS)
@preconcurrency import RelayFFI
import Darwin
import ForgeAppKit
import Foundation
import ForgeUI
import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Send files, photos or a folder with a word code to anyone on this Wi-Fi
/// (Relay M1). The receiver types the code into Relay's Receive, or runs
/// `relay receive <code>` on a computer. Rust hashes and serves what was
/// picked and decides the code; this sheet picks, advertises the code with
/// Bonjour and shows where the send is.
struct SendCodeSheet: View {
    @Bindable var model: RelayModel
    let network: RelayNetwork

    /// What the file picker is open for, if it is.
    @State private var picking: Pick?
    @State private var pickingPhotos = false
    @State private var photos: [PhotosPickerItem] = []
    /// The copies of what is being sent, kept until the next send starts.
    @State private var outgoing: [ForgePickedFile] = []
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    status
                } footer: {
                    Text("The receiver needs Relay, or the relay command on a computer, on the same Wi-Fi. Several items arrive together in one folder.")
                }
                Section("Memory") {
                    MemoryReadout()
                }
            }
            .navigationTitle("Send")
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
                    Menu {
                        Button("Files", systemImage: "doc") { picking = .files }
                            .accessibilityIdentifier("sendCode.files")
                        Button("Photos", systemImage: "photo.on.rectangle") { pickingPhotos = true }
                            .accessibilityIdentifier("sendCode.photos")
                        Button("A Folder", systemImage: "folder") { picking = .folder }
                            .accessibilityIdentifier("sendCode.folder")
                    } label: {
                        Text("Choose")
                    }
                    .disabled(model.isSendingCode)
                    .accessibilityIdentifier("sendCode.choose")
                }
            }
            .fileImporter(
                isPresented: Binding(get: { picking != nil }, set: { if !$0 { picking = nil } }),
                allowedContentTypes: picking == .folder ? [.folder] : [.item],
                allowsMultipleSelection: picking != .folder
            ) { result in
                switch result {
                case .success(let urls): start(urls)
                case .failure(let error): model.failSend(error.localizedDescription)
                }
            }
            .photosPicker(
                isPresented: $pickingPhotos, selection: $photos,
                matching: .any(of: [.images, .videos]), preferredItemEncoding: .current
            )
            .onChange(of: photos) {
                let picked = photos
                photos = []
                if !picked.isEmpty { start(photos: picked) }
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
            // Back from the background: iOS dropped the advert meanwhile.
            .onChange(of: model.returns) { network.readvertiseCode(model.codeAdvert) }
            .onDisappear {
                network.stopWatchingCodeSenders()
                if !model.isSendingCode { network.advertiseCode(nil) }
            }
            .forgeSwipeGuard(model.isSendingCode)
        }
    }

    @ViewBuilder private var status: some View {
        switch model.codeSend {
        case .idle:
            Text("Choose files, photos or a folder, and Relay shows a code for the receiver.")
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
                    .forgeForeground(.success)
                Text("\(RelayFormat.size(size)) in \(String(format: "%.1f", seconds)) s, \(String(format: "%.1f", speed)) MB/s")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("sendCode.status")
        case .failed(let reason):
            Label(reason, systemImage: "exclamationmark.triangle.fill")
                .forgeForeground(.danger)
                .accessibilityIdentifier("sendCode.status")
        }
    }

    /// Copies what was picked off the main thread (ForgeAppKit's
    /// `ForgePickedFile`: the picker's access ends with this call, and Rust
    /// reads the files for the whole send), then starts the send. The
    /// previous send's copies are deleted when the next one starts.
    private func start(_ urls: [URL]) {
        guard !urls.isEmpty else { return }
        model.preparing(name: Self.name(urls.map(\.lastPathComponent)))
        Task {
            releaseOutgoing()
            do {
                for url in urls {
                    outgoing.append(try await ForgePickedFile.copy(of: url))
                }
                model.send(paths: outgoing.map(\.url.path))
            } catch {
                releaseOutgoing()
                model.failSend("Couldn't read what you chose: \(error.localizedDescription)")
            }
        }
    }

    /// Loads the picked photos and videos as files (the originals, not
    /// re-encoded where iOS can help it), then starts the send.
    private func start(photos: [PhotosPickerItem]) {
        model.preparing(name: photos.count == 1 ? "a photo" : "\(photos.count) photos")
        Task {
            releaseOutgoing()
            do {
                var paths: [String] = []
                for item in photos {
                    if let file = try await item.loadTransferable(type: PickedPhoto.self) {
                        paths.append(file.url.path)
                    }
                }
                if paths.isEmpty {
                    model.failSend("Couldn't read the photos you chose.")
                } else {
                    model.send(paths: paths)
                }
            } catch {
                model.failSend("Couldn't read the photos: \(error.localizedDescription)")
            }
        }
    }

    /// Deletes the copies an earlier send made.
    private func releaseOutgoing() {
        outgoing.forEach { $0.release() }
        outgoing = []
        PickedPhoto.releaseAll()
    }

    private static func name(_ names: [String]) -> String {
        names.count == 1 ? names[0] : "\(names.count) items"
    }

    enum Pick {
        case files, folder
    }
}

/// A picked photo or video as a file. iOS hands over a file that lives only
/// for the import, so it is copied first, into a folder of its own that
/// keeps its name (Photos already gives each one a distinct name).
struct PickedPhoto: Transferable {
    let url: URL

    static var directory: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("relay-photos", isDirectory: true)
    }

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(importedContentType: .image) { try PickedPhoto(keeping: $0.file) }
        FileRepresentation(importedContentType: .movie) { try PickedPhoto(keeping: $0.file) }
    }

    init(keeping file: URL) throws {
        let folder = Self.directory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let kept = folder.appendingPathComponent(file.lastPathComponent)
        try FileManager.default.copyItem(at: file, to: kept)
        url = kept
    }

    /// Deletes every kept photo.
    static func releaseAll() {
        try? FileManager.default.removeItem(at: directory)
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
