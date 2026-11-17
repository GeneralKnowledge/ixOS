import OpenGenerable
import XCTest

final class JSONSchemaTests: XCTestCase {
    func testTheExampleExportsInDeclaredOrder() throws {
        let expected = """
            {
              "$schema": "https://json-schema.org/draft/2020-12/schema",
              "title": "Character",
              "description": "A fantasy RPG character",
              "type": "object",
              "properties": {
                "name": {
                  "description": "Character name",
                  "type": "string"
                },
                "class": {
                  "type": "string",
                  "enum": [
                    "Warrior",
                    "Mage",
                    "Rogue"
                  ]
                },
                "level": {
                  "type": "integer",
                  "minimum": 1,
                  "maximum": 20
                },
                "background": {
                  "type": "string"
                },
                "inventory": {
                  "type": "array",
                  "items": {
                    "description": "Something the character carries",
                    "type": "object",
                    "properties": {
                      "name": {
                        "type": "string"
                      },
                      "weight": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 50
                      },
                      "magical": {
                        "type": "boolean"
                      }
                    },
                    "required": [
                      "name",
                      "weight"
                    ],
                    "additionalProperties": false
                  },
                  "minItems": 0,
                  "maxItems": 3
                },
                "motto": {
                  "description": "A short motto",
                  "type": "string"
                }
              },
              "required": [
                "name",
                "class",
                "level",
                "background",
                "inventory"
              ],
              "additionalProperties": false
            }
            """
        XCTAssertEqual(try Fixtures.schema().jsonSchemaString, expected)
    }

    func testReferencesGoUnderDefs() throws {
        let person = DynamicGenerationSchema(
            name: "Person",
            properties: [
                .init(name: "pets", schema: .array(of: .reference(to: "Pet"))),
                .init(name: "partner", schema: .anyOf([.reference(to: "Person"), .null])),
            ])
        let pet = DynamicGenerationSchema(name: "Pet", properties: [.init(name: "name", schema: .string())])
        let exported = try GenerationSchema(root: person, dependencies: [pet]).jsonSchema()

        XCTAssertEqual(exported["properties"]?["pets"]?["items"]?["$ref"], "#/$defs/Pet")
        XCTAssertEqual(
            exported["properties"]?["partner"]?["anyOf"],
            [["$ref": "#"], ["type": "null"]])
        XCTAssertEqual(exported["$defs"]?.orderedKeys, ["Pet"])
        XCTAssertEqual(exported["$defs"]?["Pet"]?["required"], ["name"])
    }

    func testAnExportedSchemaIsValidJSON() throws {
        let text = try Fixtures.schema().jsonSchemaString
        XCTAssertNoThrow(try JSONSerialization.jsonObject(with: Data(text.utf8)))
    }
}
