// Copied into an app's Generated/ForgeFFI by scripts/generate-bindings.sh
// when its Rust crate calls `forge_runtime::log_export!()`. Do not edit the
// copy; the source of truth is crates/forge-runtime/swift/ForgeLog.swift.

/// Writes Swift log records into Forge's log pipeline, so they reach the
/// system log (`forge device logs`), the recent records kept for
/// diagnostics, and the `forge device` live stream, next to Rust's.
///
/// ```swift
/// private let log = ForgeLog("Attachments")
/// log.warn("thumbnail failed: \(error)")
/// ```
///
/// Messages are built only when their level is enabled. Never log secrets:
/// tokens, passwords, Keychain contents, message bodies.
public struct ForgeLog: Sendable {
    /// The record's target, e.g. the feature or file it came from.
    public let category: String

    public init(_ category: String) {
        self.category = category
    }

    public func trace(_ message: @autoclosure () -> String) { write(.trace, message) }
    public func debug(_ message: @autoclosure () -> String) { write(.debug, message) }
    public func info(_ message: @autoclosure () -> String) { write(.info, message) }
    public func warn(_ message: @autoclosure () -> String) { write(.warn, message) }
    public func error(_ message: @autoclosure () -> String) { write(.error, message) }

    private func write(_ level: ForgeLogLevel, _ message: () -> String) {
        guard forgeLogEnabled(level: level) else { return }
        forgeLog(level: level, category: category, message: message())
    }
}
