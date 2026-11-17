/// Why some text isn't JSON.
public struct JSONParseError: Error, Equatable, Sendable, CustomStringConvertible {
    /// The byte offset (in UTF-8) where the problem was found.
    public var offset: Int
    public var reason: String

    public var description: String { "invalid JSON at byte \(offset): \(reason)" }
}

/// A strict JSON parser that keeps object keys in order.
struct JSONParser {
    static let maximumDepth = 512

    let bytes: [UInt8]
    var index = 0
    var depth = 0

    init(_ bytes: [UInt8]) {
        self.bytes = bytes
    }

    mutating func parseDocument() throws -> GeneratedContent {
        skipWhitespace()
        let value = try parseValue()
        skipWhitespace()
        guard index == bytes.count else { throw fail("unexpected text after the value") }
        return value
    }

    func fail(_ reason: String) -> JSONParseError {
        JSONParseError(offset: index, reason: reason)
    }

    var current: UInt8? { index < bytes.count ? bytes[index] : nil }

    mutating func skipWhitespace() {
        while let byte = current, byte == 0x20 || byte == 0x0A || byte == 0x0D || byte == 0x09 {
            index += 1
        }
    }

    mutating func expect(_ literal: String, _ value: GeneratedContent) throws -> GeneratedContent {
        for byte in literal.utf8 {
            guard current == byte else { throw fail("expected \(literal)") }
            index += 1
        }
        return value
    }

    mutating func parseValue() throws -> GeneratedContent {
        guard let byte = current else { throw fail("unexpected end of text") }
        switch byte {
        case UInt8(ascii: "{"): return try parseObject()
        case UInt8(ascii: "["): return try parseArray()
        case UInt8(ascii: "\""): return GeneratedContent(kind: .string(try parseString()))
        case UInt8(ascii: "t"): return try expect("true", true)
        case UInt8(ascii: "f"): return try expect("false", false)
        case UInt8(ascii: "n"): return try expect("null", nil)
        case UInt8(ascii: "-"), UInt8(ascii: "0")...UInt8(ascii: "9"): return try parseNumber()
        default: throw fail("unexpected character")
        }
    }

    mutating func enter() throws {
        depth += 1
        guard depth <= Self.maximumDepth else { throw fail("nested more than \(Self.maximumDepth) levels") }
    }

    mutating func parseObject() throws -> GeneratedContent {
        try enter()
        defer { depth -= 1 }
        index += 1  // {
        var values: [String: GeneratedContent] = [:]
        var keys: [String] = []
        skipWhitespace()
        if current == UInt8(ascii: "}") {
            index += 1
            return GeneratedContent(kind: .structure(properties: [:], orderedKeys: []))
        }
        while true {
            skipWhitespace()
            guard current == UInt8(ascii: "\"") else { throw fail("expected a key") }
            let keyStart = index
            let key = try parseString()
            guard values[key] == nil else {
                throw JSONParseError(offset: keyStart, reason: "the key \"\(key)\" appears twice")
            }
            skipWhitespace()
            guard current == UInt8(ascii: ":") else { throw fail("expected :") }
            index += 1
            skipWhitespace()
            values[key] = try parseValue()
            keys.append(key)
            skipWhitespace()
            switch current {
            case UInt8(ascii: ","): index += 1
            case UInt8(ascii: "}"):
                index += 1
                return GeneratedContent(kind: .structure(properties: values, orderedKeys: keys))
            default: throw fail("expected , or }")
            }
        }
    }

    mutating func parseArray() throws -> GeneratedContent {
        try enter()
        defer { depth -= 1 }
        index += 1  // [
        var elements: [GeneratedContent] = []
        skipWhitespace()
        if current == UInt8(ascii: "]") {
            index += 1
            return GeneratedContent(kind: .array([]))
        }
        while true {
            skipWhitespace()
            elements.append(try parseValue())
            skipWhitespace()
            switch current {
            case UInt8(ascii: ","): index += 1
            case UInt8(ascii: "]"):
                index += 1
                return GeneratedContent(kind: .array(elements))
            default: throw fail("expected , or ]")
            }
        }
    }

    mutating func parseString() throws -> String {
        index += 1  // "
        var scalars = String.UnicodeScalarView()
        var runStart = index
        func flush(_ end: Int) {
            if end > runStart {
                scalars.append(contentsOf: String(decoding: bytes[runStart..<end], as: UTF8.self).unicodeScalars)
            }
        }
        while true {
            guard let byte = current else { throw fail("unterminated string") }
            switch byte {
            case UInt8(ascii: "\""):
                flush(index)
                index += 1
                return String(scalars)
            case UInt8(ascii: "\\"):
                flush(index)
                index += 1
                scalars.append(try parseEscape())
                runStart = index
            case 0..<0x20:
                throw fail("control character in a string")
            default:
                index += 1
            }
        }
    }

    mutating func parseEscape() throws -> Unicode.Scalar {
        guard let byte = current else { throw fail("unterminated escape") }
        index += 1
        switch byte {
        case UInt8(ascii: "\""): return "\""
        case UInt8(ascii: "\\"): return "\\"
        case UInt8(ascii: "/"): return "/"
        case UInt8(ascii: "b"): return "\u{08}"
        case UInt8(ascii: "f"): return "\u{0C}"
        case UInt8(ascii: "n"): return "\n"
        case UInt8(ascii: "r"): return "\r"
        case UInt8(ascii: "t"): return "\t"
        case UInt8(ascii: "u"):
            let high = try parseHex4()
            if (0xD800..<0xDC00).contains(high) {
                guard current == UInt8(ascii: "\\"), index + 1 < bytes.count, bytes[index + 1] == UInt8(ascii: "u")
                else { throw fail("lone surrogate") }
                index += 2
                let low = try parseHex4()
                guard (0xDC00..<0xE000).contains(low) else { throw fail("lone surrogate") }
                return Unicode.Scalar(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))!
            }
            guard let scalar = Unicode.Scalar(high) else { throw fail("lone surrogate") }
            return scalar
        default:
            index -= 1
            throw fail("unknown escape")
        }
    }

    mutating func parseHex4() throws -> UInt32 {
        guard index + 4 <= bytes.count else { throw fail("short \\u escape") }
        var value: UInt32 = 0
        for _ in 0..<4 {
            let byte = bytes[index]
            let digit: UInt8
            switch byte {
            case UInt8(ascii: "0")...UInt8(ascii: "9"): digit = byte - UInt8(ascii: "0")
            case UInt8(ascii: "a")...UInt8(ascii: "f"): digit = byte - UInt8(ascii: "a") + 10
            case UInt8(ascii: "A")...UInt8(ascii: "F"): digit = byte - UInt8(ascii: "A") + 10
            default: throw fail("bad hex digit")
            }
            value = value * 16 + UInt32(digit)
            index += 1
        }
        return value
    }

    mutating func parseNumber() throws -> GeneratedContent {
        let start = index
        func digits() -> Int {
            let from = index
            while let byte = current, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(byte) { index += 1 }
            return index - from
        }
        if current == UInt8(ascii: "-") { index += 1 }
        if current == UInt8(ascii: "0") {
            index += 1
            if let byte = current, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(byte) {
                throw fail("leading zero")
            }
        } else if digits() == 0 {
            throw fail("expected a digit")
        }
        if current == UInt8(ascii: ".") {
            index += 1
            guard digits() > 0 else { throw fail("expected a digit after .") }
        }
        if current == UInt8(ascii: "e") || current == UInt8(ascii: "E") {
            index += 1
            if current == UInt8(ascii: "+") || current == UInt8(ascii: "-") { index += 1 }
            guard digits() > 0 else { throw fail("expected a digit in the exponent") }
        }
        let text = String(decoding: bytes[start..<index], as: UTF8.self)
        guard let value = Double(text), value.isFinite else {
            throw JSONParseError(offset: start, reason: "number out of range")
        }
        return GeneratedContent(kind: .number(value))
    }
}
