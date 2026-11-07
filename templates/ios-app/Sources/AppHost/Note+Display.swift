@preconcurrency import ForgeFFI
import Foundation

// Small additions that let SwiftUI use the Rust record directly. They are
// presentation only (how a note is titled and dated on screen); rules about
// what a note may contain stay in Rust.

// UniFFI records are Equatable and Hashable already; `id` is the Rust id.
// `@retroactive`: the type and the protocol both come from other modules.
extension Note: @retroactive Identifiable {}

extension Note {
    public var createdAt: Date {
        Date(timeIntervalSince1970: TimeInterval(createdAtMs) / 1000)
    }

    /// The first line, shown as the note's title.
    public var title: String {
        lines.first ?? ""
    }

    /// The text after the first line, on one line, if there is any.
    public var preview: String? {
        let rest = lines.dropFirst().joined(separator: " ")
        return rest.isEmpty ? nil : rest
    }

    private var lines: [String] {
        text.split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }
}
