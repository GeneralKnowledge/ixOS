#if os(iOS)
@preconcurrency import RelayFFI
import Darwin
import Foundation
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
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase

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
                        .foregroundStyle(.orange)
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

    /// Copies what was picked into this app's temporary folder (the
    /// picker's access ends with this call; Rust reads the files for the
    /// whole send), then starts the send.
    private func start(_ urls: [URL]) {
        guard !urls.isEmpty else { return }
        model.preparing(name: Self.name(urls.map(\.lastPathComponent)))
        Task {
            let copied = await Task.detached(priority: .userInitiated) { () -> Result<[String], Error> in
                do {
                    let folder = try SendStaging.fresh()
                    var paths: [String] = []
                    for url in urls {
                        let scoped = url.startAccessingSecurityScopedResource()
                        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                        let target = SendStaging.unique(url.lastPathComponent, in: folder)
                        try FileManager.default.copyItem(at: url, to: target)
                        paths.append(target.path)
                    }
                    return .success(paths)
                } catch {
                    return .failure(error)
                }
            }.value
            switch copied {
            case .success(let paths): model.send(paths: paths)
            case .failure(let error): model.failSend("Couldn't read what you chose: \(error.localizedDescription)")
            }
        }
    }

    /// Loads the picked photos and videos as files (the originals, not
    /// re-encoded where iOS can help it), then starts the send.
    private func start(photos: [PhotosPickerItem]) {
        model.preparing(name: photos.count == 1 ? "a photo" : "\(photos.count) photos")
        Task {
            do {
                let folder = try SendStaging.fresh()
                var paths: [String] = []
                for item in photos {
                    guard let file = try await item.loadTransferable(type: PickedFile.self) else { continue }
                    let target = SendStaging.unique(file.url.lastPathComponent, in: folder)
                    try FileManager.default.moveItem(at: file.url, to: target)
                    paths.append(target.path)
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

    private static func name(_ names: [String]) -> String {
        names.count == 1 ? names[0] : "\(names.count) items"
    }

    enum Pick {
        case files, folder
    }
}

/// Where picked items are copied for the send: one folder in the app's
/// temporary directory, emptied for each new send.
enum SendStaging {
    static var folder: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("relay-send", isDirectory: true)
    }

    static func fresh() throws -> URL {
        let folder = folder
        try? FileManager.default.removeItem(at: folder)
        try? FileManager.default.removeItem(at: PickedFile.holding)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return folder
    }

    /// `name` in `folder`, numbered ("photo 2.jpg") if it is taken: items
    /// in one send need different names.
    static func unique(_ name: String, in folder: URL) -> URL {
        var target = folder.appendingPathComponent(name)
        let stem = (name as NSString).deletingPathExtension
        let ext = (name as NSString).pathExtension
        var n = 2
        while FileManager.default.fileExists(atPath: target.path) {
            let numbered = ext.isEmpty ? "\(stem) \(n)" : "\(stem) \(n).\(ext)"
            target = folder.appendingPathComponent(numbered)
            n += 1
        }
        return target
    }
}

/// A picked photo or video as a file. iOS hands over a file that lives only
/// for the import, so it is moved somewhere that lasts first.
struct PickedFile: Transferable {
    let url: URL

    static var holding: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("relay-photos", isDirectory: true)
    }

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(importedContentType: .image) { try PickedFile(keeping: $0.file) }
        FileRepresentation(importedContentType: .movie) { try PickedFile(keeping: $0.file) }
    }

    init(keeping file: URL) throws {
        let holding = Self.holding.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: holding, withIntermediateDirectories: true)
        let kept = holding.appendingPathComponent(file.lastPathComponent)
        try FileManager.default.copyItem(at: file, to: kept)
        url = kept
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
