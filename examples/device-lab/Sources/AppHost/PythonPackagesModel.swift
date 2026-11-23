@preconcurrency import ForgeFFI
import Foundation
import Observation

/// Fetches what installing a package needs from PyPI: its JSON, then the
/// wheel. A protocol so tests use a fake.
public protocol PackageDownloading: Sendable {
    /// The body and HTTP status of a GET.
    func data(from url: URL) async throws -> (Data, Int)
    /// Downloads to a temporary file and returns where it is.
    func download(from url: URL) async throws -> URL
}

public struct URLSessionPackageDownloader: PackageDownloading {
    public init() {}

    public func data(from url: URL) async throws -> (Data, Int) {
        let (data, response) = try await URLSession.shared.data(from: url)
        return (data, (response as? HTTPURLResponse)?.statusCode ?? 0)
    }

    public func download(from url: URL) async throws -> URL {
        let (file, response) = try await URLSession.shared.download(from: url)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else { throw URLError(.badServerResponse) }
        // URLSession deletes its file when this returns: keep a copy.
        let kept = FileManager.default.temporaryDirectory
            .appendingPathComponent("wheel-\(UUID().uuidString).whl")
        try FileManager.default.moveItem(at: file, to: kept)
        return kept
    }
}

/// Python Lab's packages (python-lab-workspace-plan.md, PL2a): pure-Python
/// wheels from PyPI installed into the app's own `site-packages`, with
/// their requirements. Rust (`packages.rs`) chooses the wheel, checks its
/// checksum, refuses compiled code and unpacks it; this downloads and
/// keeps the list.
@MainActor
@Observable
public final class PythonPackagesModel {
    /// What the person installed.
    public private(set) var installed: [InstalledPackage] = []
    /// What is built into this build of the app (numpy and the like, when
    /// the build has them); these can't be removed or replaced.
    public private(set) var bundled: [InstalledPackage] = []
    /// The package name typed in.
    public var name = ""
    public private(set) var isWorking = false
    /// What the last install did, a line per package.
    public private(set) var log: [String] = []
    public private(set) var error: String?

    /// Installing stops after this many packages (a requirement chain that
    /// runs away).
    // swift-only: Swift walks the requirements; Rust installs one wheel at a time.
    public static let maxPackages = 40

    @ObservationIgnored private let sitePackages: String
    @ObservationIgnored private let bundledSite: String?
    @ObservationIgnored private let downloader: any PackageDownloading

    public init(
        sitePackages: String, bundledSite: String? = nil,
        downloader: any PackageDownloading = URLSessionPackageDownloader()
    ) {
        self.sitePackages = sitePackages
        self.bundledSite = bundledSite
        self.downloader = downloader
        reload()
    }

    public func reload() {
        installed = packagesInstalled(sitePackages: sitePackages)
        bundled = bundledSite.map { packagesInstalled(sitePackages: $0) } ?? []
    }

    /// Installs the package typed in, or upgrades it, and then whatever it
    /// needs that isn't here yet.
    public func install() async {
        let requested = name.trimmingCharacters(in: .whitespaces)
        guard !isWorking, !requested.isEmpty else { return }
        guard packageNameValid(name: requested) else {
            error = "\(requested) isn't a package name. Names use letters, digits, - _ and ."
            return
        }
        if bundled.contains(where: { same($0.name, requested) }) {
            error = "\(requested) is built into this app, so it can't be installed or replaced here."
            return
        }
        isWorking = true
        error = nil
        log = []
        // What is here already, by name, with what it needs.
        var present: [String: InstalledPackage] = [:]
        for package in installed {
            present[packageNormalizedName(name: package.name)] = package
        }
        // Built in: never installed, and what they need is the build's job.
        let builtIn = Set(bundled.map { packageNormalizedName(name: $0.name) })
        // The one asked for is installed again (an upgrade) even if here.
        present[packageNormalizedName(name: requested)] = nil
        // Each name once, so a cycle of requirements ends.
        var visited: Set<String> = []
        var queue = [requested]
        var count = 0
        while let next = queue.first {
            queue.removeFirst()
            let key = packageNormalizedName(name: next)
            guard visited.insert(key).inserted, !builtIn.contains(key) else { continue }
            if let here = present[key] {
                // Not downloaded again, but what it needs is checked: a
                // requirement an earlier install couldn't finish is retried.
                queue += here.requires
                continue
            }
            count += 1
            if count > Self.maxPackages {
                error = "Stopped after \(Self.maxPackages) packages: \(requested) needs too many."
                break
            }
            do {
                let package = try await installOne(next)
                log.append("Installed \(package.name) \(package.version)")
                queue += package.requires
            } catch {
                self.error = next == requested
                    ? Self.message(error)
                    : "\(requested) needs \(next), which couldn't be installed: \(Self.message(error))"
                break
            }
        }
        if error == nil { name = "" }
        reload()
        isWorking = false
    }

    private func installOne(_ package: String) async throws -> InstalledPackage {
        guard let url = URL(string: "https://pypi.org/pypi/\(package)/json") else {
            throw PackageError.Invalid(reason: "\(package) isn't a package name.")
        }
        let (data, status) = try await downloader.data(from: url)
        if status == 404 { throw PackageError.NotFound(name: package) }
        guard status == 200 else {
            throw PackageError.Storage(reason: "PyPI answered \(status). Try again later.")
        }
        let choice = try packageChooseWheel(name: package, pypiJson: String(decoding: data, as: UTF8.self))
        guard let wheelURL = URL(string: choice.url) else {
            throw PackageError.Invalid(reason: "PyPI gave a link that isn't one.")
        }
        let file = try await downloader.download(from: wheelURL)
        defer { try? FileManager.default.removeItem(at: file) }
        // Unpacking is file work: off the main actor.
        let site = sitePackages
        let sha = choice.sha256
        return try await Task.detached {
            try packageInstallWheel(wheel: file.path, sha256: sha, sitePackages: site)
        }.value
    }

    /// Removes a package the person installed. One already imported keeps
    /// working until Device Lab restarts.
    public func remove(_ package: InstalledPackage) {
        // An install in progress counts on what is here when it started.
        guard !isWorking else { return }
        do {
            try packageUninstall(sitePackages: sitePackages, name: package.name)
            error = nil
            log = ["Removed \(package.name) \(package.version)"]
        } catch {
            self.error = Self.message(error)
        }
        reload()
    }

    private func same(_ a: String, _ b: String) -> Bool {
        packageNormalizedName(name: a) == packageNormalizedName(name: b)
    }

    nonisolated static func message(_ error: Error) -> String {
        switch error as? PackageError {
        case .NotFound(let name): "PyPI has no package called \(name)."
        case .NotPure(let reason), .Invalid(let reason), .Storage(let reason): reason
        case nil: error.localizedDescription
        }
    }
}
