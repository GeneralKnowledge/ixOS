/// One piece of a schema, built at run time.
///
/// The shape follows Apple's `DynamicGenerationSchema`: objects with
/// properties, lists, choices (`anyOf`), references to named schemas and
/// null. Build the pieces, then hand the root and any named pieces it refers
/// to to `GenerationSchema(root:dependencies:)`, which checks them.
///
/// ```swift
/// let character = DynamicGenerationSchema(
///     name: "Character",
///     description: "A fantasy RPG character",
///     properties: [
///         .init(name: "name", schema: .string(description: "Character name")),
///         .init(name: "class", schema: .anyOf(["Warrior", "Mage", "Rogue"])),
///         .init(name: "level", schema: .integer(range: 1...20)),
///     ])
/// let schema = try GenerationSchema(root: character)
/// ```
public struct DynamicGenerationSchema: Sendable, Equatable {
    indirect enum Node: Sendable, Equatable {
        case string
        /// Exactly one of these strings.
        case choices([String])
        case integer(minimum: Int?, maximum: Int?)
        case number(minimum: Double?, maximum: Double?)
        case boolean
        case null
        case array(element: DynamicGenerationSchema, minimum: Int?, maximum: Int?)
        case object([Property])
        /// A value that matches at least one of these.
        case anyOf([DynamicGenerationSchema])
        /// The named schema with this name.
        case reference(String)
    }

    var node: Node
    /// Set on schemas others can refer to by name (objects and choices,
    /// usually). Unnamed schemas are used where they are written.
    public var name: String?
    public var description: String?

    init(_ node: Node, name: String? = nil, description: String? = nil) {
        self.node = node
        self.name = name
        self.description = description
    }

    /// A property of an object.
    public struct Property: Sendable, Equatable {
        public var name: String
        public var description: String?
        public var schema: DynamicGenerationSchema
        /// Whether the key may be left out. A present value must still fit
        /// `schema`; to allow `null`, write `.anyOf([schema, .null])`.
        public var isOptional: Bool

        public init(
            name: String, description: String? = nil, schema: DynamicGenerationSchema, isOptional: Bool = false
        ) {
            self.name = name
            self.description = description
            self.schema = schema
            self.isOptional = isOptional
        }
    }
}

// MARK: - Apple's initialisers

extension DynamicGenerationSchema {
    /// An object whose properties are written in this order.
    public init(name: String, description: String? = nil, properties: [Property]) {
        self.init(.object(properties), name: name, description: description)
    }

    /// One of a fixed list of strings.
    public init(name: String, description: String? = nil, anyOf choices: [String]) {
        self.init(.choices(choices), name: name, description: description)
    }

    /// A value that matches one of these schemas.
    public init(name: String, description: String? = nil, anyOf choices: [DynamicGenerationSchema]) {
        self.init(.anyOf(choices), name: name, description: description)
    }

    /// A list of `element`, with between `minimumElements` and
    /// `maximumElements` items when they are given.
    public init(arrayOf element: DynamicGenerationSchema, minimumElements: Int? = nil, maximumElements: Int? = nil) {
        self.init(.array(element: element, minimum: minimumElements, maximum: maximumElements))
    }

    /// The named schema `name`, passed to `GenerationSchema` as the root or a
    /// dependency. References make recursive schemas possible.
    public init(referenceTo name: String) {
        self.init(.reference(name))
    }
}

// MARK: - Shorthand

extension DynamicGenerationSchema {
    /// Any text.
    public static func string(description: String? = nil) -> Self {
        Self(.string, description: description)
    }

    /// One of these strings. Give it a `name` to refer to it from elsewhere.
    public static func anyOf(_ choices: [String], name: String? = nil, description: String? = nil) -> Self {
        Self(.choices(choices), name: name, description: description)
    }

    /// A value that matches one of these schemas.
    public static func anyOf(
        _ schemas: [DynamicGenerationSchema], name: String? = nil, description: String? = nil
    ) -> Self {
        Self(.anyOf(schemas), name: name, description: description)
    }

    /// A whole number, within `range` when it is given.
    public static func integer(description: String? = nil, range: ClosedRange<Int>? = nil) -> Self {
        Self(.integer(minimum: range?.lowerBound, maximum: range?.upperBound), description: description)
    }

    /// A whole number of at least `minimum` (and at most `maximum`).
    public static func integer(description: String? = nil, minimum: Int, maximum: Int? = nil) -> Self {
        Self(.integer(minimum: minimum, maximum: maximum), description: description)
    }

    /// A whole number of at most `maximum`.
    public static func integer(description: String? = nil, maximum: Int) -> Self {
        Self(.integer(minimum: nil, maximum: maximum), description: description)
    }

    /// A number, within `range` when it is given.
    public static func number(description: String? = nil, range: ClosedRange<Double>? = nil) -> Self {
        Self(.number(minimum: range?.lowerBound, maximum: range?.upperBound), description: description)
    }

    /// A number of at least `minimum` (and at most `maximum`).
    public static func number(description: String? = nil, minimum: Double, maximum: Double? = nil) -> Self {
        Self(.number(minimum: minimum, maximum: maximum), description: description)
    }

    /// A number of at most `maximum`.
    public static func number(description: String? = nil, maximum: Double) -> Self {
        Self(.number(minimum: nil, maximum: maximum), description: description)
    }

    /// `true` or `false`.
    public static func boolean(description: String? = nil) -> Self {
        Self(.boolean, description: description)
    }

    /// Only `null`. Useful inside `anyOf`.
    public static var null: Self { Self(.null) }

    /// A list of `element`, with a number of items within `count` when it is
    /// given.
    public static func array(
        of element: DynamicGenerationSchema, description: String? = nil, count: ClosedRange<Int>? = nil
    ) -> Self {
        Self(.array(element: element, minimum: count?.lowerBound, maximum: count?.upperBound), description: description)
    }

    /// An object. Give it a `name` to refer to it from elsewhere.
    public static func object(
        name: String? = nil, description: String? = nil, properties: [Property]
    ) -> Self {
        Self(.object(properties), name: name, description: description)
    }

    /// The named schema `name`.
    public static func reference(to name: String) -> Self {
        Self(.reference(name))
    }
}
