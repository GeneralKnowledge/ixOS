/// JSON Schema (draft 2020-12) export.
///
/// Properties keep their declared order, which matters to grammar
/// generators (llama.cpp's, for one) that write keys in schema order. Objects
/// are closed (`additionalProperties: false`), as in guided generation. Named
/// schemas that something refers to go under `$defs`; a reference to the
/// root is `#`.
extension GenerationSchema {
    public static let jsonSchemaDialect = "https://json-schema.org/draft/2020-12/schema"

    /// The schema as a JSON Schema document.
    public func jsonSchema() -> GeneratedContent {
        var members: [(String, GeneratedContent)] = [("$schema", .init(stringLiteral: Self.jsonSchemaDialect))]
        if let name = root.name { members.append(("title", .init(stringLiteral: name))) }
        members += export(root)

        let referenced = referencedNames().filter { $0 != root.name }
        if !referenced.isEmpty {
            members.append(
                ("$defs", GeneratedContent(properties: referenced.map { ($0, GeneratedContent(properties: export(named[$0]!))) })))
        }
        return GeneratedContent(properties: members)
    }

    /// `jsonSchema()` as indented JSON text.
    public var jsonSchemaString: String { jsonSchema().prettyJSONString }

    /// Names that some reference points to, in the order first met.
    func referencedNames() -> [String] {
        var order: [String] = []
        var seen = Set<String>()
        func visit(_ schema: DynamicGenerationSchema) {
            if case .reference(let name) = schema.node, seen.insert(name).inserted {
                order.append(name)
                if let target = named[name] { visit(target) }
            }
            for (child, _) in schema.children(at: "") { visit(child) }
        }
        visit(root)
        return order
    }

    func export(_ schema: DynamicGenerationSchema, descriptionOverride: String? = nil) -> [(String, GeneratedContent)] {
        var out: [(String, GeneratedContent)] = []
        func put(_ key: String, _ value: GeneratedContent) { out.append((key, value)) }
        func string(_ text: String) -> GeneratedContent { GeneratedContent(kind: .string(text)) }
        func number(_ value: Double) -> GeneratedContent { GeneratedContent(kind: .number(value)) }

        if let description = descriptionOverride ?? schema.description { put("description", string(description)) }

        switch schema.node {
        case .string:
            put("type", "string")
        case .choices(let choices):
            put("type", "string")
            put("enum", GeneratedContent(kind: .array(choices.map(string))))
        case .integer(let minimum, let maximum):
            put("type", "integer")
            if let minimum { put("minimum", number(Double(minimum))) }
            if let maximum { put("maximum", number(Double(maximum))) }
        case .number(let minimum, let maximum):
            put("type", "number")
            if let minimum { put("minimum", number(minimum)) }
            if let maximum { put("maximum", number(maximum)) }
        case .boolean:
            put("type", "boolean")
        case .null:
            put("type", "null")
        case .array(let element, let minimum, let maximum):
            put("type", "array")
            put("items", GeneratedContent(properties: export(element)))
            if let minimum { put("minItems", number(Double(minimum))) }
            if let maximum { put("maxItems", number(Double(maximum))) }
        case .object(let properties):
            put("type", "object")
            put(
                "properties",
                GeneratedContent(
                    properties: properties.map {
                        ($0.name, GeneratedContent(properties: export($0.schema, descriptionOverride: $0.description)))
                    }))
            let required = properties.filter { !$0.isOptional }.map { string($0.name) }
            put("required", GeneratedContent(kind: .array(required)))
            put("additionalProperties", false)
        case .anyOf(let schemas):
            put("anyOf", GeneratedContent(kind: .array(schemas.map { GeneratedContent(properties: export($0)) })))
        case .reference(let name):
            put("$ref", string(name == root.name ? "#" : "#/$defs/" + name))
        }
        return out
    }
}
