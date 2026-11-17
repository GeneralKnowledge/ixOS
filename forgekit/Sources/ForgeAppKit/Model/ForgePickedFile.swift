// A file the person picked, copied where the app owns it. Foundation only,
// so the copy and its clean-up are tested on Linux.

import Foundation

/// The app's own copy of a file the person picked (with `fileImporter`, the
/// document picker or a drop).
///
/// A picked URL is security-scoped: the app may read it only briefly, so a
/// file Rust reads later (an upload, a send that takes minutes) needs a copy.
/// Copying a large video takes seconds, so `copy(of:)` runs off the main
/// thread and the screen never freezes (Relay, friction log A40). Each copy
/// sits in a folder of its own under the temporary directory, keeping the
/// picked file's name and extension. `release()` deletes it when the app is
/// done; iOS clears anything left in the temporary directory.
///
///     let file = try await ForgePickedFile.copy(of: url)
///     defer { file.release() }
///     try await core.send(path: file.url.path)
///
/// When Rust reads the file straight away, inside the call, no copy is
/// needed: hold the security scope around that call instead.
public struct ForgePickedFile: Hashable, Sendable {
    /// The copy, readable until `release()`.
    public let url: URL

    /// The picked file's name, extension included.
    public var name: String { url.lastPathComponent }

    /// Where copies go unless the app says otherwise:
    /// `<tmp>/ForgePickedFiles`.
    public static var defaultDirectory: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("ForgePickedFiles", isDirectory: true)
    }

    /// Copies `picked` off the main thread, holding its security scope while
    /// it reads. Throws if the file can't be read or the copy can't be
    /// written; nothing is left behind then.
    public static func copy(of picked: URL, into directory: URL? = nil) async throws -> ForgePickedFile {
        let directory = directory ?? defaultDirectory
        return try await Task.detached(priority: .userInitiated) {
            try copyNow(of: picked, into: directory)
        }.value
    }

    /// Deletes the copy and its folder. Safe to call more than once.
    public func release() {
        try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
    }

    static func copyNow(of picked: URL, into directory: URL) throws -> ForgePickedFile {
        #if canImport(Darwin)
            let scoped = picked.startAccessingSecurityScopedResource()
            defer { if scoped { picked.stopAccessingSecurityScopedResource() } }
        #endif
        let fileManager = FileManager.default
        let folder = directory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try fileManager.createDirectory(at: folder, withIntermediateDirectories: true)
        let copy = folder.appendingPathComponent(picked.lastPathComponent)
        do {
            try fileManager.copyItem(at: picked, to: copy)
        } catch {
            try? fileManager.removeItem(at: folder)
            throw error
        }
        return ForgePickedFile(url: copy)
    }
}
