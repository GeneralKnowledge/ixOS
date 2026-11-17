import OpenGenerable
import XCTest

final class ValidationTests: XCTestCase {
    let valid = """
        {"name": "Ilsa", "class": "Mage", "level": 7, "background": "Raised by owls",
         "inventory": [{"name": "Staff", "weight": 2.5, "magical": true}, {"name": "Bread", "weight": 0.2}]}
        """

    func testAValidAnswerPasses() throws {
        let schema = try Fixtures.schema()
        let content = try schema.content(fromJSON: valid)
        XCTAssertEqual(content["name"]?.stringValue, "Ilsa")
        XCTAssertEqual(content["level"]?.intValue, 7)
        XCTAssertEqual(content["inventory"]?.elements?.count, 2)
        XCTAssertEqual(content["inventory"]?.elements?[0]["magical"]?.boolValue, true)
        XCTAssertEqual(content.orderedKeys, ["name", "class", "level", "background", "inventory"])
    }

    func testInvalidValuesCannotPass() throws {
        let schema = try Fixtures.schema()
        let bad = """
            {"name": 3, "class": "Bard", "level": 21, "inventory": [{"name": "Anvil", "weight": 80}],
             "hat": "big"}
            """
        XCTAssertThrowsError(try schema.content(fromJSON: bad)) { error in
            let violations = (error as? ValidationError)?.violations ?? []
            XCTAssertEqual(
                violations,
                [
                    .init(path: "/name", problem: .wrongType(expected: "string", found: "number")),
                    .init(path: "/class", problem: .notAChoice("Bard", choices: ["Warrior", "Mage", "Rogue"])),
                    .init(path: "/level", problem: .outOfRange(21, minimum: 1, maximum: 20)),
                    .init(path: "", problem: .missingProperty("background")),
                    .init(path: "/inventory/0/weight", problem: .outOfRange(80, minimum: 0, maximum: 50)),
                    .init(path: "", problem: .unexpectedProperty("hat")),
                ])
        }
    }

    func testIntegersMustBeWhole() throws {
        let schema = try GenerationSchema(
            root: .object(name: "R", properties: [.init(name: "n", schema: .integer())]))
        XCTAssertEqual(schema.violations(in: ["n": 2.5]), [.init(path: "/n", problem: .notWhole(2.5))])
        XCTAssertEqual(schema.violations(in: ["n": 2.0]), [])
        XCTAssertEqual(
            schema.violations(in: ["n": "2"]), [.init(path: "/n", problem: .wrongType(expected: "integer", found: "string"))])
    }

    func testListLengths() throws {
        let schema = try GenerationSchema(
            root: .object(name: "R", properties: [.init(name: "tags", schema: .array(of: .string(), count: 1...2))]))
        XCTAssertEqual(
            schema.violations(in: ["tags": []]), [.init(path: "/tags", problem: .wrongCount(0, minimum: 1, maximum: 2))])
        XCTAssertEqual(schema.violations(in: ["tags": ["a", "b"]]), [])
        XCTAssertEqual(
            schema.violations(in: ["tags": ["a", "b", "c"]]),
            [.init(path: "/tags", problem: .wrongCount(3, minimum: 1, maximum: 2))])
        XCTAssertEqual(
            schema.violations(in: ["tags": ["a", 1]]),
            [.init(path: "/tags/1", problem: .wrongType(expected: "string", found: "number"))])
    }

    func testOptionalPropertiesMayBeMissingOrNull() throws {
        let schema = try Fixtures.schema()
        var content = try GeneratedContent(json: valid)
        XCTAssertEqual(schema.violations(in: content), [])
        guard case .structure(var properties, var keys) = content.kind else { return XCTFail() }
        properties["motto"] = nil
        keys.append("motto")
        properties["motto"] = GeneratedContent(kind: .null)
        content.kind = .structure(properties: properties, orderedKeys: keys)
        XCTAssertEqual(schema.violations(in: content), [])

        // A required property that is null counts as missing.
        properties["name"] = GeneratedContent(kind: .null)
        content.kind = .structure(properties: properties, orderedKeys: keys)
        XCTAssertEqual(schema.violations(in: content), [.init(path: "", problem: .missingProperty("name"))])
    }

    func testAnyOfSchemasAndNull() throws {
        let schema = try GenerationSchema(
            root: .object(
                name: "R",
                properties: [.init(name: "x", schema: .anyOf([.integer(range: 0...9), .anyOf(["none"]), .null]))]))
        XCTAssertEqual(schema.violations(in: ["x": 4]), [])
        XCTAssertEqual(schema.violations(in: ["x": "none"]), [])
        XCTAssertEqual(schema.violations(in: ["x": nil]), [])
        XCTAssertEqual(schema.violations(in: ["x": 12]), [.init(path: "/x", problem: .noAlternativeFits)])
        XCTAssertEqual(schema.violations(in: ["x": "some"]), [.init(path: "/x", problem: .noAlternativeFits)])
    }

    func testRecursiveValues() throws {
        let node = DynamicGenerationSchema(
            name: "Node",
            properties: [
                .init(name: "label", schema: .string()),
                .init(name: "children", schema: .array(of: .reference(to: "Node"))),
            ])
        let schema = try GenerationSchema(root: node)
        let tree = try GeneratedContent(
            json: #"{"label":"a","children":[{"label":"b","children":[{"label":"c","children":[]}]}]}"#)
        XCTAssertEqual(schema.violations(in: tree), [])
        let broken = try GeneratedContent(
            json: #"{"label":"a","children":[{"label":"b","children":[{"label":1,"children":[]}]}]}"#)
        XCTAssertEqual(
            schema.violations(in: broken),
            [.init(path: "/children/0/children/0/label", problem: .wrongType(expected: "string", found: "number"))])
    }

    func testPointerPathsEscapeSlashes() throws {
        let schema = try GenerationSchema(
            root: .object(name: "R", properties: [.init(name: "a/b~c", schema: .boolean())]))
        XCTAssertEqual(
            schema.violations(in: ["a/b~c": "yes"]),
            [.init(path: "/a~1b~0c", problem: .wrongType(expected: "boolean", found: "string"))])
    }

    func testDecodingIntoAStruct() throws {
        struct Character: Decodable, Equatable {
            struct Item: Decodable, Equatable {
                var name: String
                var weight: Double
                var magical: Bool?
            }
            var name: String
            var `class`: String
            var level: Int
            var inventory: [Item]
        }
        let content = try Fixtures.schema().content(fromJSON: valid)
        let character = try content.decode(Character.self)
        XCTAssertEqual(character.level, 7)
        XCTAssertEqual(character.inventory[1], .init(name: "Bread", weight: 0.2, magical: nil))
    }

    func testTextThatIsNotJSONIsAParseError() throws {
        let schema = try Fixtures.schema()
        XCTAssertThrowsError(try schema.content(fromJSON: "Sure! Here is a character: {")) {
            XCTAssertTrue($0 is JSONParseError)
        }
    }
}
