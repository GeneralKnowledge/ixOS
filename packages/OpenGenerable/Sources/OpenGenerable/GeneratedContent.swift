import Foundation

/// A value a model wrote: JSON, with object keys kept in the order they
/// were written. Mirrors Apple's `GeneratedContent`.
public struct GeneratedContent: Sendable, Equatable {
    public enum Kind: Sendable, Equatable {
        case null
        case bool(Bool)
        case number(Double)
        case string(String)
        case array([GeneratedContent])
        /// An object. `orderedKeys` is the order the keys were written in.
        case structure(properties: [String: GeneratedContent], orderedKeys: [String])
    }

    public var kind: Kind

    public init(kind: Kind) {
        self.kind = kind
    }

    /// An object with these properties, in this order. A later duplicate key
    /// replaces the earlier value.
    public init(properties: [(String, GeneratedContent)]) {
        var values: [String: GeneratedContent] = [:]
        var keys: [String] = []
        for (key, value) in properties {
            if values.updateValue(value, forKey: key) == nil { keys.append(key) }
        }
        self.kind = .structure(properties: values, orderedKeys: keys)
    }

    /// Parses strict JSON (RFC 8259). Duplicate keys are an error.
    public init(json: String) throws {
        var parser = JSONParser(Array(json.utf8))
        self = try parser.parseDocument()
    }
}

// MARK: - Reading values

extension GeneratedContent {
    /// The value of `property`, if this is an object that has it.
    public subscript(property: String) -> GeneratedContent? {
        guard case .structure(let properties, _) = kind else { return nil }
        return properties[property]
    }

    public var stringValue: String? {
        if case .string(let value) = kind { value } else { nil }
    }

    public var doubleValue: Double? {
        if case .number(let value) = kind { value } else { nil }
    }

    /// The number, if it is whole and fits in an `Int`.
    public var intValue: Int? {
        guard case .number(let value) = kind, value.isWhole, let int = Int(exactly: value) else { return nil }
        return int
    }

    public var boolValue: Bool? {
        if case .bool(let value) = kind { value } else { nil }
    }

    public var elements: [GeneratedContent]? {
        if case .array(let value) = kind { value } else { nil }
    }

    public var isNull: Bool { kind == .null }

    /// The object's keys in the order they were written.
    public var orderedKeys: [String]? {
        if case .structure(_, let keys) = kind { keys } else { nil }
    }

    /// Decodes the value into a `Decodable` type, through its JSON.
    public func decode<T: Decodable>(_ type: T.Type = T.self, decoder: JSONDecoder = JSONDecoder()) throws -> T {
        try decoder.decode(type, from: Data(jsonString.utf8))
    }
}

extension Double {
    var isWhole: Bool { isFinite && rounded(.towardZero) == self }
}

// MARK: - Writing JSON

extension GeneratedContent {
    /// Compact JSON, keys in their written order.
    public var jsonString: String {
        var out = ""
        write(to: &out, indent: nil, depth: 0)
        return out
    }

    /// JSON indented by two spaces, keys in their written order.
    public var prettyJSONString: String {
        var out = ""
        write(to: &out, indent: "  ", depth: 0)
        return out
    }

    func write(to out: inout String, indent: String?, depth: Int) {
        func newline(_ level: Int) {
            guard let indent else { return }
            out += "\n" + String(repeating: indent, count: level)
        }
        switch kind {
        case .null:
            out += "null"
        case .bool(let value):
            out += value ? "true" : "false"
        case .number(let value):
            out += Self.format(value)
        case .string(let value):
            Self.writeString(value, to: &out)
        case .array(let elements):
            guard !elements.isEmpty else {
                out += "[]"
                return
            }
            out += "["
            for (index, element) in elements.enumerated() {
                if index > 0 { out += "," }
                newline(depth + 1)
                element.write(to: &out, indent: indent, depth: depth + 1)
            }
            newline(depth)
            out += "]"
        case .structure(let properties, let keys):
            guard !keys.isEmpty else {
                out += "{}"
                return
            }
            out += "{"
            for (index, key) in keys.enumerated() {
                if index > 0 { out += "," }
                newline(depth + 1)
                Self.writeString(key, to: &out)
                out += indent == nil ? ":" : ": "
                (properties[key] ?? GeneratedContent(kind: .null)).write(to: &out, indent: indent, depth: depth + 1)
            }
            newline(depth)
            out += "}"
        }
    }

    static func format(_ value: Double) -> String {
        guard value.isFinite else { return "null" }
        if value.isWhole, abs(value) < 1e15 { return String(Int64(value)) }
        return "\(value)"
    }

    static func writeString(_ value: String, to out: inout String) {
        out += "\""
        for scalar in value.unicodeScalars {
            switch scalar {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            case "\u{08}": out += "\\b"
            case "\u{0C}": out += "\\f"
            case _ where scalar.value < 0x20:
                let hex = String(scalar.value, radix: 16)
                out += "\\u" + String(repeating: "0", count: 4 - hex.count) + hex
            default:
                out.unicodeScalars.append(scalar)
            }
        }
        out += "\""
    }
}

// MARK: - Literals

extension GeneratedContent: ExpressibleByStringLiteral, ExpressibleByIntegerLiteral, ExpressibleByFloatLiteral,
    ExpressibleByBooleanLiteral, ExpressibleByNilLiteral, ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral
{
    public init(stringLiteral value: String) { kind = .string(value) }
    public init(integerLiteral value: Int) { kind = .number(Double(value)) }
    public init(floatLiteral value: Double) { kind = .number(value) }
    public init(booleanLiteral value: Bool) { kind = .bool(value) }
    public init(nilLiteral: ()) { kind = .null }
    public init(arrayLiteral elements: GeneratedContent...) { kind = .array(elements) }
    public init(dictionaryLiteral elements: (String, GeneratedContent)...) { self.init(properties: elements) }
}

extension GeneratedContent: CustomStringConvertible {
    public var description: String { jsonString }
}
