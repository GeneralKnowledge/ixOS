// Forge native services: app directories and file import. Source of truth: crates/forge-services/swift/.

import Foundation
import SwiftUI
import UniformTypeIdentifiers

public enum ForgeFiles {
    /// The app's directories, created if missing. Under `swift run` and
    /// `swift test` on a Mac, Application Support and Caches get a
    /// per-process subfolder so test runs do not share the user's folders.
    public static func appDirectories(fileManager: FileManager = .default) throws -> AppDirectories {
        func directory(_ kind: FileManager.SearchPathDirectory) throws -> URL {
            var url = try fileManager.url(for: kind, in: .userDomainMask, appropriateFor: nil, create: true)
            #if os(macOS)
                if kind != .documentDirectory {
                    let name = Bundle.main.bundleIdentifier ?? ProcessInfo.processInfo.processName
                    url = url.appendingPathComponent(name, isDirectory: true)
                    try fileManager.createDirectory(at: url, withIntermediateDirectories: true)
                }
            #endif
            return url
        }
        return AppDirectories(
            documents: try directory(.documentDirectory).path,
            applicationSupport: try directory(.applicationSupportDirectory).path,
            caches: try directory(.cachesDirectory).path,
            temporary: fileManager.temporaryDirectory.path)
    }

    /// Copies a file the user picked (from `fileImporter`, a document picker
    /// or a drop) into `directory`, by default `<tmp>/ForgeImports`. The
    /// picker's access to the original ends soon after; the copy is the app's
    /// own, so Rust can read it by path. A name that is taken gets a unique
    /// prefix.
    public static func importCopy(of url: URL, into directory: URL? = nil) throws -> URL {
        let fileManager = FileManager.default
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let inbox =
            directory
            ?? fileManager.temporaryDirectory.appendingPathComponent("ForgeImports", isDirectory: true)
        try fileManager.createDirectory(at: inbox, withIntermediateDirectories: true)
        var destination = inbox.appendingPathComponent(url.lastPathComponent)
        if fileManager.fileExists(atPath: destination.path) {
            destination = inbox.appendingPathComponent("\(UUID().uuidString)-\(url.lastPathComponent)")
        }
        try fileManager.copyItem(at: url, to: destination)
        return destination
    }
}

extension View {
    /// `fileImporter` that hands back a copy the app owns
    /// (`ForgeFiles.importCopy`), ready to pass to Rust by path.
    public func forgeFileImporter(
        isPresented: Binding<Bool>,
        allowedContentTypes: [UTType],
        onImport: @escaping (Result<URL, Error>) -> Void
    ) -> some View {
        fileImporter(isPresented: isPresented, allowedContentTypes: allowedContentTypes) { result in
            onImport(result.flatMap { url in Result(catching: { try ForgeFiles.importCopy(of: url) }) })
        }
    }
}
