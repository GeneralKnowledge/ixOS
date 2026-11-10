// The line under a form field. Foundation only, so a view model (which
// imports no SwiftUI) can produce it, for example from a Rust validation
// result, and a Linux test can check it.

/// Help or validation text shown under a `ForgeField`.
public struct ForgeFieldMessage: Hashable, Sendable {
    public enum Kind: String, Hashable, Sendable {
        case help
        case success
        case warning
        case error
    }

    public var kind: Kind
    public var text: String

    public init(_ kind: Kind, _ text: String) {
        self.kind = kind
        self.text = text
    }

    public static func help(_ text: String) -> ForgeFieldMessage { ForgeFieldMessage(.help, text) }
    public static func success(_ text: String) -> ForgeFieldMessage { ForgeFieldMessage(.success, text) }
    public static func warning(_ text: String) -> ForgeFieldMessage { ForgeFieldMessage(.warning, text) }
    public static func error(_ text: String) -> ForgeFieldMessage { ForgeFieldMessage(.error, text) }

    /// The tone the message is drawn in.
    public var tone: ForgeTone {
        switch kind {
        case .help: .neutral
        case .success: .success
        case .warning: .warning
        case .error: .danger
        }
    }

    public var isError: Bool { kind == .error }
}
