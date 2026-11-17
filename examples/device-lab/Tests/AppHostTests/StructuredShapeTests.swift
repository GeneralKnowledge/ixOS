import AppHost
import Foundation
import XCTest

/// `StructuredShape`, the no-macro stand-in for `@Generable`: Foundation
/// only, so these run wherever Swift does.
final class StructuredShapeTests: XCTestCase {
    struct Dish: StructuredAnswer, Equatable {
        var name: String
        var minutes: Int
        var tags: [String]
        var vegetarian: Bool?

        static let shape = StructuredShape.object(
            name: "Dish",
            fields: [
                StructuredField("name", .string()),
                StructuredField("minutes", .integer(range: 1...60)),
                StructuredField("tags", .array(of: .string(choices: ["quick", "cold"]), count: 0...2)),
                StructuredField("vegetarian", .boolean(), optional: true),
            ])
    }

    struct Clash: StructuredAnswer {
        var inner: [String: String]
        static let shape = StructuredShape.object(
            name: "Clash",
            fields: [StructuredField("inner", .object(name: "Clash", fields: []))])
    }

    struct NotObject: StructuredAnswer {
        static let shape = StructuredShape.string()
        init(from decoder: Decoder) throws {}
    }

    func testTheAnswerDecodesIntoTheStruct() throws {
        let dish = try Dish.decode(json: #"{"name":"Salad","minutes":10,"tags":["quick"]}"#)
        XCTAssertEqual(dish, Dish(name: "Salad", minutes: 10, tags: ["quick"], vegetarian: nil))
        XCTAssertThrowsError(try Dish.decode(json: #"{"name":"Salad""#))
    }

    func testShapesAreCheckedBeforeTheModel() throws {
        XCTAssertNoThrow(try Dish.validateShape())
        XCTAssertThrowsError(try Clash.validateShape()) { error in
            XCTAssertEqual(error as? StructuredShapeError, .duplicateName("Clash"))
        }
        XCTAssertThrowsError(try NotObject.validateShape()) { error in
            XCTAssertEqual(error as? StructuredShapeError, .rootNotObject)
        }
    }
}
