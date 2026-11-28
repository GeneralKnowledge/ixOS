@preconcurrency import BusinessPreviewFFI
@preconcurrency import ForgeKit
import ForgeAppKit
import Foundation
import Observation
#if canImport(FoundationNetworking)
    import FoundationNetworking
#endif

/// The prospects list and its imports. Every rule (what a pack must hold,
/// what is kept, what the preview may show) is the Rust core's; this asks it
/// and shows what it says.
///
/// It imports no SwiftUI, so `forge check swift` compiles and tests it on
/// Linux.
@Observable
@MainActor
public final class ProspectsModel {
    public private(set) var prospects: [Prospect]
    /// Something to tell the person, or ask them.
    public var alert: ForgeAlert<Action>?
    /// True while a pack from Files is being read.
    public private(set) var importing = false
    /// Prospects being deleted, shown as pending rows.
    public private(set) var deleting: Set<String> = []
    public private(set) var feedback = ForgeFeedback()

    public let core: BusinessPreviewCore

    public enum Action: Equatable {
        case delete(id: String)
    }

    public init(core: BusinessPreviewCore) {
        self.core = core
        prospects = core.prospects()
    }

    /// Re-reads the list from Rust. Cheap: it never waits for a command.
    public func reload() {
        prospects = core.prospects()
    }

    /// Imports a pack picked in Files (a zip or a profile.json).
    public func importPicked(_ url: URL) {
        importing = true
        Task {
            defer { importing = false }
            do {
                let file = try await ForgePickedFile.copy(of: url)
                defer { file.release() }
                try await finishImport(at: file.url)
            } catch {
                alert = .failure(error)
                feedback.play(.failure)
            }
        }
    }

    /// Downloads a pack from `address` and imports it. Returns the failure
    /// to show in the form, or nil when it was imported.
    public func download(address: String) async -> ForgeFailure? {
        do {
            let checked = try businessPreviewPackAddress(text: address)
            guard let url = URL(string: checked) else {
                return ForgeFailure(title: "Can't Use That Address", message: "Check the address and try again.")
            }
            let file = try await Self.fetch(url)
            defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
            try await finishImport(at: file)
            return nil
        } catch {
            feedback.play(.failure)
            return ForgeFailure.isCancellation(error) ? nil : ForgeFailure(error)
        }
    }

    private func finishImport(at file: URL) async throws {
        let imported = try await core.importPack(path: file.path)
        reload()
        feedback.play(.success)
        if imported.replaced {
            alert = ForgeAlert(
                title: "\(imported.prospect.name) Updated",
                message: "The new pack replaced the one on this iPhone.",
                buttons: [.ok]
            )
        }
    }

    /// Saves the download to a folder of its own, so the pack keeps a name
    /// Rust can read. It stops as soon as the download outgrows the largest
    /// pack Rust takes, and it follows a redirect only to another https
    /// address.
    private static func fetch(_ url: URL) async throws -> URL {
        #if canImport(Darwin)
            let limit = businessPreviewMaxPackBytes()
            let (bytes, response) = try await URLSession.shared.bytes(from: url, delegate: HTTPSOnlyRedirects())
            if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                throw DownloadError.status(http.statusCode)
            }
            if response.expectedContentLength > 0, UInt64(response.expectedContentLength) > limit {
                throw DownloadError.tooLarge
            }
            let folder = FileManager.default.temporaryDirectory
                .appendingPathComponent("BusinessPreviewDownloads", isDirectory: true)
                .appendingPathComponent(UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let name = url.lastPathComponent.isEmpty ? "pack.zip" : url.lastPathComponent
            let file = folder.appendingPathComponent(name)
            do {
                FileManager.default.createFile(atPath: file.path, contents: nil)
                let handle = try FileHandle(forWritingTo: file)
                defer { try? handle.close() }
                var buffer = Data()
                buffer.reserveCapacity(64 * 1024)
                var received: UInt64 = 0
                for try await byte in bytes {
                    buffer.append(byte)
                    if buffer.count == 64 * 1024 {
                        received += UInt64(buffer.count)
                        if received > limit { throw DownloadError.tooLarge }
                        try handle.write(contentsOf: buffer)
                        buffer.removeAll(keepingCapacity: true)
                    }
                }
                received += UInt64(buffer.count)
                if received > limit { throw DownloadError.tooLarge }
                try handle.write(contentsOf: buffer)
            } catch {
                try? FileManager.default.removeItem(at: folder)
                throw error
            }
            return file
        #else
            throw DownloadError.unsupported
        #endif
    }

    public func askToDelete(_ prospect: Prospect) {
        alert = .confirm(
            "Delete \(prospect.name)?",
            message: "Its pack is removed from this iPhone. Send it again to show it again.",
            confirm: "Delete",
            action: .delete(id: prospect.id),
            identifier: "prospects.delete",
            destructive: true
        )
    }

    public func handle(_ action: Action) {
        switch action {
        case .delete(let id):
            deleting.insert(id)
            Task {
                defer { deleting.remove(id) }
                do {
                    try await core.delete(id: id)
                    reload()
                } catch {
                    alert = .failure(error)
                    feedback.play(.failure)
                }
            }
        }
    }
}

enum DownloadError: ForgeDescribedError {
    case status(Int)
    case tooLarge
    case unsupported

    var failure: ForgeFailure {
        switch self {
        case .status(404):
            ForgeFailure(title: "Pack Not Found", message: "Nothing is at that address. Check it and try again.")
        case .status(let code):
            ForgeFailure(title: "Download Failed", message: "The server answered \(code). Try again in a moment.", isRetryable: true)
        case .tooLarge:
            ForgeFailure(title: "Pack Too Large", message: "The file at that address is larger than the preview takes.")
        case .unsupported:
            ForgeFailure(title: "Can't Download Here", message: "Downloading packs needs an iPhone.")
        }
    }
}

#if canImport(Darwin)
    /// Lets a pack download follow a redirect only to another https address:
    /// the address the person typed was checked, so every hop must be too.
    private final class HTTPSOnlyRedirects: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
        func urlSession(
            _ session: URLSession,
            task: URLSessionTask,
            willPerformHTTPRedirection response: HTTPURLResponse,
            newRequest request: URLRequest
        ) async -> URLRequest? {
            request.url?.scheme?.lowercased() == "https" ? request : nil
        }
    }
#endif

/// What the preview screens show for a business, loaded from Rust.
@Observable
@MainActor
public final class PreviewModel {
    public enum State {
        case loading
        case ready(BusinessView)
        case failed(ForgeFailure)
    }

    public private(set) var state: State = .loading
    public let id: String
    private let core: BusinessPreviewCore

    public init(core: BusinessPreviewCore, id: String) {
        self.core = core
        self.id = id
    }

    public func load() async {
        do {
            state = .ready(try await core.business(id: id))
        } catch {
            state = .failed(ForgeFailure(error))
        }
    }

    /// The concept label, even before the business loads.
    public static let conceptLabel = businessPreviewConceptLabel()
}

extension Prospect: Identifiable {}

extension BusinessPreviewError: ForgeDescribedError {
    public var failure: ForgeFailure {
        switch self {
        case .NotAPack:
            ForgeFailure(title: "Not a Profile Pack", message: "Pick the zip or profile.json the extractor made for a business.")
        case .Invalid(let problems):
            ForgeFailure(title: "This Profile Can't Be Shown", message: Self.list(problems))
        case .TooLarge(let what, _):
            ForgeFailure(title: "Pack Too Large", message: "\(what.prefix(1).uppercased() + what.dropFirst()) is larger than the preview takes.")
        case .BadImage(let path, let reason):
            ForgeFailure(title: "A Picture Can't Be Used", message: "\(path): \(reason).")
        case .HashMismatch(let path):
            ForgeFailure(title: "Pack Damaged", message: "\(path) doesn't match the pack's list of hashes. Send the pack again.")
        case .BadAddress(let reason):
            ForgeFailure(title: "Can't Use That Address", message: reason.prefix(1).uppercased() + reason.dropFirst() + ".")
        case .NotFound:
            ForgeFailure(title: "Not on This iPhone", message: "This business has been deleted.")
        case .Damaged:
            ForgeFailure(title: "Saved Copy Damaged", message: "This business's files changed after they were imported, so it isn't shown. Import its pack again.")
        case .InUse:
            ForgeFailure(title: "Already Open", message: "Business Preview is open in another copy of the app.")
        case .Corrupt:
            ForgeFailure(title: "Prospects Can't Be Read", message: "The saved list is damaged.")
        case .NewerData:
            ForgeFailure(title: "Update the App", message: "The prospects were saved by a newer version of the app.")
        case .Storage:
            ForgeFailure(title: "Couldn't Save", message: "Try again. If it keeps happening, check the iPhone has free space.", isRetryable: true)
        case .Stopped:
            ForgeFailure(title: "Stopped", message: "The app is closing.")
        }
    }

    public var isCancellation: Bool {
        if case .Stopped = self { return true }
        return false
    }

    /// The first few problems, one per line.
    private static func list(_ problems: [String]) -> String {
        // cut: the alert shows three problems; the extractor's validator lists them all.
        let shown = problems.prefix(3).joined(separator: "\n")
        let more = problems.count > 3 ? "\nand \(problems.count - 3) more." : ""
        return shown + more
    }
}
