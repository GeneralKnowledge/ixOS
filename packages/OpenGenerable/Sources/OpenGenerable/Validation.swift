/// One way a value breaks its schema.
public struct SchemaViolation: Error, Equatable, Sendable, CustomStringConvertible {
    /// Where, as a JSON Pointer (RFC 6901): `""` is the whole value,
    /// `/party/0/level` the level of the first party member.
    public var path: String
    public var problem: Problem

    public init(path: String, problem: Problem) {
        self.path = path
        self.problem = problem
    }

    public enum Problem: Equatable, Sendable {
        /// The value has the wrong JSON type.
        case wrongType(expected: String, found: String)
        /// A required property is missing (or null).
        case missingProperty(String)
        /// A property the schema doesn't list.
        case unexpectedProperty(String)
        /// A string that isn't one of the choices.
        case notAChoice(String, choices: [String])
        /// A number outside its bounds, or a fraction where a whole number
        /// belongs.
        case outOfRange(Double, minimum: Double?, maximum: Double?)
        case notWhole(Double)
        /// A list with too few or too many items.
        case wrongCount(Int, minimum: Int?, maximum: Int?)
        /// No alternative of an `anyOf` fits.
        case noAlternativeFits
    }

    public var description: String {
        let place = path.isEmpty ? "the value" : path
        switch problem {
        case .wrongType(let expected, let found): return "\(place): expected \(expected), found \(found)"
        case .missingProperty(let name): return "\(place): missing \(name)"
        case .unexpectedProperty(let name): return "\(place): unexpected property \(name)"
        case .notAChoice(let value, let choices):
            return "\(place): \"\(value)\" is not one of \(choices.joined(separator: ", "))"
        case .outOfRange(let value, let minimum, let maximum):
            return "\(place): \(GeneratedContent.format(value)) is outside "
                + "\(minimum.map(GeneratedContent.format) ?? "")...\(maximum.map(GeneratedContent.format) ?? "")"
        case .notWhole(let value): return "\(place): \(GeneratedContent.format(value)) is not a whole number"
        case .wrongCount(let count, let minimum, let maximum):
            return "\(place): \(count) items, expected \(minimum.map(String.init) ?? "")...\(maximum.map(String.init) ?? "")"
        case .noAlternativeFits: return "\(place): fits none of the alternatives"
        }
    }
}

/// A value that broke its schema, with every violation found.
public struct ValidationError: Error, Equatable, Sendable, CustomStringConvertible {
    public var violations: [SchemaViolation]

    public var description: String {
        violations.map(\.description).joined(separator: "; ")
    }
}

extension GenerationSchema {
    /// Every way `content` breaks the schema; empty when it fits.
    public func violations(in content: GeneratedContent) -> [SchemaViolation] {
        var found: [SchemaViolation] = []
        check(content, against: root, path: "", into: &found)
        return found
    }

    /// Throws `ValidationError` unless `content` fits the schema.
    public func validate(_ content: GeneratedContent) throws {
        let found = violations(in: content)
        if !found.isEmpty { throw ValidationError(violations: found) }
    }

    /// Parses a model's JSON answer and checks it. Throws `JSONParseError`
    /// for text that isn't JSON and `ValidationError` for JSON that doesn't
    /// fit.
    public func content(fromJSON json: String) throws -> GeneratedContent {
        let content = try GeneratedContent(json: json)
        try validate(content)
        return content
    }

    func check(
        _ content: GeneratedContent, against schema: DynamicGenerationSchema, path: String,
        into found: inout [SchemaViolation]
    ) {
        func report(_ problem: SchemaViolation.Problem) {
            found.append(SchemaViolation(path: path, problem: problem))
        }
        func wrongType(_ expected: String) {
            report(.wrongType(expected: expected, found: content.typeName))
        }

        switch (resolve(schema).node, content.kind) {
        case (.reference, _):
            // `resolve` ends on a reference only for a loop, which
            // `GenerationSchema.init` refuses.
            report(.noAlternativeFits)

        case (.string, .string):
            break
        case (.string, _):
            wrongType("string")

        case (.choices(let choices), .string(let value)):
            if !choices.contains(value) { report(.notAChoice(value, choices: choices)) }
        case (.choices, _):
            wrongType("string")

        case (.integer(let minimum, let maximum), .number(let value)):
            if !value.isWhole {
                report(.notWhole(value))
            } else if let minimum, value < Double(minimum) {
                report(.outOfRange(value, minimum: Double(minimum), maximum: maximum.map(Double.init)))
            } else if let maximum, value > Double(maximum) {
                report(.outOfRange(value, minimum: minimum.map(Double.init), maximum: Double(maximum)))
            }
        case (.integer, _):
            wrongType("integer")

        case (.number(let minimum, let maximum), .number(let value)):
            if let minimum, value < minimum {
                report(.outOfRange(value, minimum: minimum, maximum: maximum))
            } else if let maximum, value > maximum {
                report(.outOfRange(value, minimum: minimum, maximum: maximum))
            }
        case (.number, _):
            wrongType("number")

        case (.boolean, .bool):
            break
        case (.boolean, _):
            wrongType("boolean")

        case (.null, .null):
            break
        case (.null, _):
            wrongType("null")

        case (.array(let element, let minimum, let maximum), .array(let elements)):
            if let minimum, elements.count < minimum {
                report(.wrongCount(elements.count, minimum: minimum, maximum: maximum))
            } else if let maximum, elements.count > maximum {
                report(.wrongCount(elements.count, minimum: minimum, maximum: maximum))
            }
            for (index, item) in elements.enumerated() {
                check(item, against: element, path: path + "/\(index)", into: &found)
            }
        case (.array, _):
            wrongType("array")

        case (.object(let properties), .structure(let values, let keys)):
            for property in properties {
                let childPath = path + "/" + Self.escapePointer(property.name)
                guard let value = values[property.name], !value.isNull || Self.allowsNull(property.schema, in: self)
                else {
                    if !property.isOptional { report(.missingProperty(property.name)) }
                    continue
                }
                check(value, against: property.schema, path: childPath, into: &found)
            }
            let listed = Set(properties.map(\.name))
            for key in keys where !listed.contains(key) {
                report(.unexpectedProperty(key))
            }
        case (.object, _):
            wrongType("object")

        case (.anyOf(let schemas), _):
            let fits = schemas.contains { alternative in
                var inner: [SchemaViolation] = []
                check(content, against: alternative, path: path, into: &inner)
                return inner.isEmpty
            }
            if !fits { report(.noAlternativeFits) }
        }
    }

    /// Whether `null` fits `schema` itself (so a null property is a value,
    /// not a missing one).
    static func allowsNull(_ schema: DynamicGenerationSchema, in root: GenerationSchema) -> Bool {
        var found: [SchemaViolation] = []
        root.check(nil, against: schema, path: "", into: &found)
        return found.isEmpty
    }

    static func escapePointer(_ token: String) -> String {
        token.replacingOccurrences(of: "~", with: "~0").replacingOccurrences(of: "/", with: "~1")
    }
}

extension GeneratedContent {
    var typeName: String {
        switch kind {
        case .null: "null"
        case .bool: "boolean"
        case .number: "number"
        case .string: "string"
        case .array: "array"
        case .structure: "object"
        }
    }
}
