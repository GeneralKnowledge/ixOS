/// A checked schema: a root and the named schemas it refers to.
///
/// Creating one checks the whole tree, so a schema that reaches a model or
/// validator is known to be usable: every reference resolves, names are
/// unique, bounds make sense and some finite value fits.
public struct GenerationSchema: Sendable, Equatable {
    public let root: DynamicGenerationSchema
    /// Every named schema, root and dependencies included, by name.
    let named: [String: DynamicGenerationSchema]

    /// - Parameters:
    ///   - root: what the model writes.
    ///   - dependencies: named schemas that `root` (or another dependency)
    ///     refers to with `DynamicGenerationSchema(referenceTo:)`.
    public init(root: DynamicGenerationSchema, dependencies: [DynamicGenerationSchema] = []) throws {
        var named: [String: DynamicGenerationSchema] = [:]
        var order: [String] = []

        func register(_ schema: DynamicGenerationSchema, at path: String) throws {
            if let name = schema.name {
                if let existing = named[name] {
                    guard existing == schema else { throw SchemaError.duplicateName(name) }
                } else {
                    named[name] = schema
                    order.append(name)
                }
            }
            try schema.checkShape(at: path)
            for (child, childPath) in schema.children(at: path) {
                try register(child, at: childPath)
            }
        }

        try register(root, at: root.name ?? "root")
        for (index, dependency) in dependencies.enumerated() {
            guard let name = dependency.name else { throw SchemaError.unnamedDependency(index: index) }
            try register(dependency, at: name)
        }

        // Every reference names something.
        func checkReferences(_ schema: DynamicGenerationSchema) throws {
            if case .reference(let target) = schema.node, named[target] == nil {
                throw SchemaError.unresolvedReference(target)
            }
            for (child, _) in schema.children(at: "") { try checkReferences(child) }
        }
        try checkReferences(root)
        for dependency in dependencies { try checkReferences(dependency) }

        // Some finite value fits each named schema and the root: a required
        // property that refers back to its own object (with no way out
        // through an optional, an empty list or another choice) would make
        // the model write forever.
        let finite = Self.finiteNames(named)
        for name in order where !finite.contains(name) {
            throw SchemaError.noFiniteValue(name)
        }
        if !root.isFinite(finite) { throw SchemaError.noFiniteValue(root.name ?? "root") }

        self.root = root
        self.named = named
    }

    /// The named schema `name`, if the schema has one.
    public func schema(named name: String) -> DynamicGenerationSchema? { named[name] }

    func resolve(_ schema: DynamicGenerationSchema) -> DynamicGenerationSchema {
        var current = schema
        var seen = Set<String>()
        while case .reference(let target) = current.node, seen.insert(target).inserted, let next = named[target] {
            current = next
        }
        return current
    }

    /// Names whose schemas have a finite value, found by growing the set
    /// until it stops changing.
    static func finiteNames(_ named: [String: DynamicGenerationSchema]) -> Set<String> {
        var finite = Set<String>()
        var changed = true
        while changed {
            changed = false
            for (name, schema) in named where !finite.contains(name) && schema.isFinite(finite) {
                finite.insert(name)
                changed = true
            }
        }
        return finite
    }
}

/// Why a schema can't be used.
public enum SchemaError: Error, Equatable, Sendable, CustomStringConvertible {
    /// Two different schemas share a name.
    case duplicateName(String)
    /// A dependency has no name, so nothing can refer to it.
    case unnamedDependency(index: Int)
    /// A reference names no schema.
    case unresolvedReference(String)
    /// An object lists the same property twice.
    case duplicateProperty(String, path: String)
    /// A list of choices, or `anyOf`, is empty.
    case noChoices(path: String)
    /// A minimum is above its maximum, or a length is negative.
    case invalidBounds(path: String)
    /// No finite value fits: it refers back to itself with no way out.
    case noFiniteValue(String)

    public var description: String {
        switch self {
        case .duplicateName(let name): "two different schemas are named \(name)"
        case .unnamedDependency(let index): "dependency \(index) has no name"
        case .unresolvedReference(let name): "nothing is named \(name)"
        case .duplicateProperty(let name, let path): "\(path) has two properties named \(name)"
        case .noChoices(let path): "\(path) has no choices"
        case .invalidBounds(let path): "\(path) has a minimum above its maximum, or a negative length"
        case .noFiniteValue(let name): "\(name) refers to itself with no way to end"
        }
    }
}

extension DynamicGenerationSchema {
    /// The schemas directly inside this one, with a path for messages.
    func children(at path: String) -> [(DynamicGenerationSchema, String)] {
        switch node {
        case .array(let element, _, _):
            [(element, path + "[]")]
        case .object(let properties):
            properties.map { ($0.schema, path + "." + $0.name) }
        case .anyOf(let schemas):
            schemas.enumerated().map { ($1, path + "|\($0)") }
        case .string, .choices, .integer, .number, .boolean, .null, .reference:
            []
        }
    }

    /// Checks this schema on its own, without its children.
    func checkShape(at path: String) throws {
        switch node {
        case .choices(let choices) where choices.isEmpty:
            throw SchemaError.noChoices(path: path)
        case .anyOf(let schemas) where schemas.isEmpty:
            throw SchemaError.noChoices(path: path)
        case .integer(let minimum?, let maximum?) where minimum > maximum:
            throw SchemaError.invalidBounds(path: path)
        case .number(let minimum, let maximum):
            if minimum?.isNaN == true || maximum?.isNaN == true { throw SchemaError.invalidBounds(path: path) }
            if let minimum, let maximum, minimum > maximum { throw SchemaError.invalidBounds(path: path) }
        case .array(_, let minimum, let maximum):
            if (minimum ?? 0) < 0 || (maximum ?? 0) < 0 { throw SchemaError.invalidBounds(path: path) }
            if let minimum, let maximum, minimum > maximum { throw SchemaError.invalidBounds(path: path) }
        case .object(let properties):
            var seen = Set<String>()
            for property in properties where !seen.insert(property.name).inserted {
                throw SchemaError.duplicateProperty(property.name, path: path)
            }
        default:
            break
        }
    }

    /// Whether some finite value fits, given the names already known to.
    func isFinite(_ finite: Set<String>) -> Bool {
        switch node {
        case .string, .choices, .integer, .number, .boolean, .null:
            true
        case .reference(let name):
            finite.contains(name)
        case .array(let element, let minimum, _):
            (minimum ?? 0) == 0 || element.isFinite(finite)
        case .object(let properties):
            properties.allSatisfy { $0.isOptional || $0.schema.isFinite(finite) }
        case .anyOf(let schemas):
            schemas.contains { $0.isFinite(finite) }
        }
    }
}
