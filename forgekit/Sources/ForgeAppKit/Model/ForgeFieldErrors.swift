// A form's validation result, field by field. Foundation only (ForgeUI's
// ForgeFieldMessage is too), so the model builds it from Rust's validation
// errors and a Linux test checks it.

import ForgeUI
import Foundation

/// Which fields of a form are wrong and why, keyed by the field's name
/// (the same names Rust's `forge_app::validation` uses). Read one field's
/// message with a subscript and hand it to `ForgeField`:
///
///     ForgeField("Title", message: model.errors["title"]) { TextField(...) }
///
/// Built from Rust's `FieldError` list in one line:
///
///     errors = ForgeFieldErrors(fields.map { ($0.field, $0.message) })
public struct ForgeFieldErrors: Hashable, Sendable {
    private var messages: [String: String]
    /// Field names in the order they were reported, so the first problem
    /// can be announced or focused.
    public private(set) var fields: [String]

    public init() {
        messages = [:]
        fields = []
    }

    /// `(field, message)` pairs. A field reported twice keeps its first
    /// message.
    public init<S: Sequence>(_ pairs: S) where S.Element == (String, String) {
        self.init()
        for (field, message) in pairs { add(field, message) }
    }

    public var isEmpty: Bool { fields.isEmpty }

    /// The field's error as a `ForgeField` message, or nil if it is fine.
    public subscript(field: String) -> ForgeFieldMessage? {
        messages[field].map { .error($0) }
    }

    /// The field's error, or `fallback` (usually help text) if it is fine.
    public func message(for field: String, otherwise fallback: ForgeFieldMessage?) -> ForgeFieldMessage? {
        self[field] ?? fallback
    }

    /// Records a problem with `field`, unless it already has one.
    public mutating func add(_ field: String, _ message: String) {
        guard messages[field] == nil else { return }
        messages[field] = message
        fields.append(field)
    }

    /// Forgets `field`'s problem (the person has edited it).
    public mutating func clear(_ field: String) {
        messages[field] = nil
        fields.removeAll { $0 == field }
    }

    /// Every problem as one sentence per field, for an alert or VoiceOver.
    public var summary: String {
        fields.compactMap { messages[$0] }.joined(separator: "\n")
    }
}

extension ForgeFieldErrors: ExpressibleByDictionaryLiteral {
    public init(dictionaryLiteral elements: (String, String)...) {
        self.init(elements)
    }
}
