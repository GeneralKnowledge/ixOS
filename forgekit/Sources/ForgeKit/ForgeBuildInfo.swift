import Foundation

/// Which build of the app is running: the version and build number, and for
/// a build made by `forge publish ios`, the commit, branch and time it was
/// built. These match the build's row on the Install page, so the person
/// holding the phone can check which build they have
/// (docs/over-the-air.md).
public struct ForgeBuildInfo: Equatable, Sendable {
    /// CFBundleShortVersionString, such as `1.0.0`.
    public let version: String
    /// CFBundleVersion. `forge publish ios` makes it unique per build
    /// (`1.29911107`).
    public let build: String
    /// The short commit, with `+changes` when the checkout had uncommitted
    /// changes (`ForgeBuildCommit`).
    public let commit: String?
    /// The branch it was built from (`ForgeBuildBranch`).
    public let branch: String?
    /// When it was built, such as `2026-11-14 14:57 UTC` (`ForgeBuildDate`).
    public let date: String?

    /// The running app's build.
    public static let current = ForgeBuildInfo(infoDictionary: Bundle.main.infoDictionary ?? [:])

    public init(infoDictionary info: [String: Any]) {
        func text(_ key: String) -> String? {
            (info[key] as? String).flatMap { $0.isEmpty ? nil : $0 }
        }
        version = text("CFBundleShortVersionString") ?? "?"
        build = text("CFBundleVersion") ?? "?"
        commit = text("ForgeBuildCommit")
        branch = text("ForgeBuildBranch")
        date = text("ForgeBuildDate")
    }

    /// `1.0.0 (1.29911107)`.
    public var versionLine: String { "\(version) (\(build))" }

    /// `main at cf57f4e, 2026-11-14 14:57 UTC`, or nil for a build that
    /// wasn't published (installed with `forge run ios`, say).
    public var sourceLine: String? {
        var parts: [String] = []
        switch (branch, commit) {
        case let (b?, c?): parts.append("\(b) at \(c)")
        case let (b?, nil): parts.append(b)
        case let (nil, c?): parts.append(c)
        case (nil, nil): break
        }
        if let date { parts.append(date) }
        return parts.isEmpty ? nil : parts.joined(separator: ", ")
    }
}
