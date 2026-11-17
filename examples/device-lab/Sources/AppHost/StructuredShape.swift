import Foundation

#if canImport(FoundationModels)
    import FoundationModels
#endif

/// The shape of a typed answer, written as a plain value instead of with
/// Apple's `@Generable` macro.
///
/// `@Generable` needs a compiler plugin that only ships inside Xcode, so it
/// can't be expanded by a Linux build. This gives the same guided
/// generation without a macro: describe the fields once, conform an ordinary
/// `Decodable` struct to `StructuredAnswer`, and the shape becomes Apple's
/// `DynamicGenerationSchema` at run time; the model's answer decodes into
/// the struct with `JSONDecoder`. The shape itself is Foundation only, so it
/// builds and tests anywhere.
///
/// Lives in the AI Lab until a second app needs it.
public indirect enum StructuredShape: Sendable, Equatable {
    /// Text; `choices` limits it to one of these exact strings.
    case string(description: String? = nil, choices: [String]? = nil)
    /// A whole number, optionally within `range`.
    case integer(description: String? = nil, range: ClosedRange<Int>? = nil)
    case number(description: String? = nil)
    case boolean(description: String? = nil)
    /// A list, optionally with `count` elements.
    case array(of: StructuredShape, description: String? = nil, count: ClosedRange<Int>? = nil)
    /// Named fields, in the order the model should write them.
    case object(name: String, description: String? = nil, fields: [StructuredField])

    /// The shape's own description, for the field that holds it.
    var about: String? {
        switch self {
        case .string(let d, _), .integer(let d, _), .number(let d), .boolean(let d),
            .array(_, let d, _), .object(_, let d, _):
            d
        }
    }

    /// The names of every object in the shape, outermost first. Apple's
    /// schema needs them unique.
    var objectNames: [String] {
        switch self {
        case .object(let name, _, let fields):
            [name] + fields.flatMap(\.shape.objectNames)
        case .array(let element, _, _):
            element.objectNames
        default:
            []
        }
    }
}

public struct StructuredField: Sendable, Equatable {
    /// The JSON key; it must match the `Decodable` property.
    public var name: String
    public var shape: StructuredShape
    /// Whether the model may leave it out (decode it as an optional).
    public var optional: Bool

    public init(_ name: String, _ shape: StructuredShape, optional: Bool = false) {
        self.name = name
        self.shape = shape
        self.optional = optional
    }
}

/// A typed answer without `@Generable`: a `Decodable` struct and its shape.
public protocol StructuredAnswer: Decodable {
    static var shape: StructuredShape { get }
}

public enum StructuredShapeError: Error, Equatable {
    /// Two objects share a name.
    case duplicateName(String)
    /// The root must be an object.
    case rootNotObject
}

extension StructuredAnswer {
    /// Decodes the model's answer (the JSON Apple's guided generation
    /// writes).
    public static func decode(json: String) throws -> Self {
        try JSONDecoder().decode(Self.self, from: Data(json.utf8))
    }

    /// Checks the shape before it reaches the model.
    public static func validateShape() throws {
        guard case .object = shape else { throw StructuredShapeError.rootNotObject }
        var seen = Set<String>()
        for name in shape.objectNames where !seen.insert(name).inserted {
            throw StructuredShapeError.duplicateName(name)
        }
    }
}

#if canImport(FoundationModels)
    @available(iOS 26.0, macOS 26.0, *)
    extension StructuredShape {
        /// Apple's run-time schema for this shape. `path` names a list of
        /// choices after the field that holds it.
        func dynamicSchema(path: String = "answer") -> DynamicGenerationSchema {
            switch self {
            case .string(_, let choices?):
                return DynamicGenerationSchema(name: path, anyOf: choices)
            case .string:
                return DynamicGenerationSchema(type: String.self)
            case .integer(_, let range?):
                return DynamicGenerationSchema(type: Int.self, guides: [.range(range)])
            case .integer:
                return DynamicGenerationSchema(type: Int.self)
            case .number:
                return DynamicGenerationSchema(type: Double.self)
            case .boolean:
                return DynamicGenerationSchema(type: Bool.self)
            case .array(let element, _, let count):
                return DynamicGenerationSchema(
                    arrayOf: element.dynamicSchema(path: path + "_item"), minimumElements: count?.lowerBound,
                    maximumElements: count?.upperBound)
            case .object(let name, let description, let fields):
                return DynamicGenerationSchema(
                    name: name, description: description,
                    properties: fields.map { field in
                        DynamicGenerationSchema.Property(
                            name: field.name, description: field.shape.about,
                            schema: field.shape.dynamicSchema(path: name + "_" + field.name),
                            isOptional: field.optional)
                    })
            }
        }
    }

    @available(iOS 26.0, macOS 26.0, *)
    extension StructuredAnswer {
        /// The schema to pass to `LanguageModelSession`.
        public static func generationSchema() throws -> GenerationSchema {
            try validateShape()
            return try GenerationSchema(root: shape.dynamicSchema(), dependencies: [])
        }
    }
#endif
