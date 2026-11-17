import OpenGenerable
import XCTest

/// The schema from the README: a fantasy character with a nested
/// inventory.
enum Fixtures {
    static let item = DynamicGenerationSchema(
        name: "Item",
        description: "Something the character carries",
        properties: [
            .init(name: "name", schema: .string()),
            .init(name: "weight", schema: .number(range: 0...50)),
            .init(name: "magical", schema: .boolean(), isOptional: true),
        ])

    static let character = DynamicGenerationSchema(
        name: "Character",
        description: "A fantasy RPG character",
        properties: [
            .init(name: "name", schema: .string(description: "Character name")),
            .init(name: "class", schema: .anyOf(["Warrior", "Mage", "Rogue"])),
            .init(name: "level", schema: .integer(range: 1...20)),
            .init(name: "background", schema: .string()),
            .init(name: "inventory", schema: .array(of: item, count: 0...3)),
            .init(name: "motto", description: "A short motto", schema: .string(), isOptional: true),
        ])

    static func schema() throws -> GenerationSchema {
        try GenerationSchema(root: character)
    }
}

final class SchemaTests: XCTestCase {
    func testTheExampleSchemaIsValid() throws {
        let schema = try Fixtures.schema()
        XCTAssertEqual(schema.root.name, "Character")
        XCTAssertEqual(schema.schema(named: "Item"), Fixtures.item)
    }

    func testAppleShapedInitialisersBuildTheSameSchema() throws {
        let classes = DynamicGenerationSchema(name: "Class", anyOf: ["Warrior", "Mage"])
        let party = DynamicGenerationSchema(
            name: "Party",
            properties: [
                .init(name: "classes", schema: DynamicGenerationSchema(arrayOf: classes, minimumElements: 1))
            ])
        XCTAssertNoThrow(try GenerationSchema(root: party))
    }

    func testTwoDifferentSchemasCannotShareAName() {
        let a = DynamicGenerationSchema(name: "Thing", properties: [.init(name: "a", schema: .string())])
        let b = DynamicGenerationSchema(name: "Thing", properties: [.init(name: "b", schema: .string())])
        let root = DynamicGenerationSchema(
            name: "Root", properties: [.init(name: "a", schema: a), .init(name: "b", schema: b)])
        XCTAssertThrowsError(try GenerationSchema(root: root)) {
            XCTAssertEqual($0 as? SchemaError, .duplicateName("Thing"))
        }
    }

    func testTheSameSchemaMayBeUsedTwice() {
        let root = DynamicGenerationSchema(
            name: "Trade",
            properties: [.init(name: "given", schema: Fixtures.item), .init(name: "taken", schema: Fixtures.item)])
        XCTAssertNoThrow(try GenerationSchema(root: root))
    }

    func testReferencesMustResolve() {
        let root = DynamicGenerationSchema(
            name: "Root", properties: [.init(name: "x", schema: .reference(to: "Missing"))])
        XCTAssertThrowsError(try GenerationSchema(root: root)) {
            XCTAssertEqual($0 as? SchemaError, .unresolvedReference("Missing"))
        }
    }

    func testDependenciesNeedNames() {
        XCTAssertThrowsError(try GenerationSchema(root: Fixtures.character, dependencies: [.string()])) {
            XCTAssertEqual($0 as? SchemaError, .unnamedDependency(index: 0))
        }
    }

    func testShapeMistakesAreCaught() {
        func error(_ property: DynamicGenerationSchema) -> SchemaError? {
            let root = DynamicGenerationSchema(name: "Root", properties: [.init(name: "x", schema: property)])
            do {
                _ = try GenerationSchema(root: root)
                return nil
            } catch {
                return error as? SchemaError
            }
        }
        XCTAssertEqual(error(.anyOf([String]())), .noChoices(path: "Root.x"))
        XCTAssertEqual(error(.anyOf([DynamicGenerationSchema]())), .noChoices(path: "Root.x"))
        XCTAssertEqual(error(.integer(minimum: 5, maximum: 1)), .invalidBounds(path: "Root.x"))
        XCTAssertEqual(error(.number(minimum: 2, maximum: 1)), .invalidBounds(path: "Root.x"))
        XCTAssertEqual(
            error(DynamicGenerationSchema(arrayOf: .string(), minimumElements: -1)), .invalidBounds(path: "Root.x"))
        XCTAssertEqual(
            error(.object(properties: [.init(name: "a", schema: .string()), .init(name: "a", schema: .string())])),
            .duplicateProperty("a", path: "Root.x"))
        XCTAssertEqual(error(.array(of: .anyOf(["only"]), count: 0...1)), nil)
    }

    func testRecursiveSchemasNeedAWayToEnd() throws {
        // A tree whose children are a list: an empty list ends it.
        let node = DynamicGenerationSchema(
            name: "Node",
            properties: [
                .init(name: "label", schema: .string()),
                .init(name: "children", schema: .array(of: .reference(to: "Node"))),
            ])
        XCTAssertNoThrow(try GenerationSchema(root: node))

        // A required child of the same type never ends.
        let endless = DynamicGenerationSchema(
            name: "Endless", properties: [.init(name: "next", schema: .reference(to: "Endless"))])
        XCTAssertThrowsError(try GenerationSchema(root: endless)) {
            XCTAssertEqual($0 as? SchemaError, .noFiniteValue("Endless"))
        }

        // The same child made optional, or nullable, ends.
        let optional = DynamicGenerationSchema(
            name: "List", properties: [.init(name: "next", schema: .reference(to: "List"), isOptional: true)])
        XCTAssertNoThrow(try GenerationSchema(root: optional))
        let nullable = DynamicGenerationSchema(
            name: "Chain", properties: [.init(name: "next", schema: .anyOf([.reference(to: "Chain"), .null]))])
        XCTAssertNoThrow(try GenerationSchema(root: nullable))
    }

    func testMutualReferencesThroughDependencies() throws {
        let person = DynamicGenerationSchema(
            name: "Person",
            properties: [
                .init(name: "name", schema: .string()),
                .init(name: "pets", schema: .array(of: .reference(to: "Pet"))),
            ])
        let pet = DynamicGenerationSchema(
            name: "Pet",
            properties: [
                .init(name: "name", schema: .string()),
                .init(name: "owner", schema: .reference(to: "Person"), isOptional: true),
            ])
        let schema = try GenerationSchema(root: person, dependencies: [pet])
        XCTAssertEqual(schema.schema(named: "Pet"), pet)
    }
}
