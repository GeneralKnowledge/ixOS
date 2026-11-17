import OpenGenerable
import XCTest

final class JSONTests: XCTestCase {
    func testRoundTripKeepsKeyOrder() throws {
        let text = #"{"z":1,"a":[true,false,null],"m":{"y":"é\n\"q\"","b":-0.5,"big":1e+20}}"#
        let content = try GeneratedContent(json: text)
        XCTAssertEqual(content.orderedKeys, ["z", "a", "m"])
        XCTAssertEqual(content.jsonString, text)
        XCTAssertEqual(try GeneratedContent(json: content.prettyJSONString), content)
    }

    func testEscapes() throws {
        let content = try GeneratedContent(json: #""\u00e9\ud83d\ude00\/\t\u0001""#)
        XCTAssertEqual(content.stringValue, "é😀/\t\u{01}")
        XCTAssertEqual(content.jsonString, "\"é😀/\\t\\u0001\"")
    }

    func testStrictness() {
        let rejected = [
            "", "{", "[1,]", "{\"a\":1,}", "01", "1.", ".5", "+1", "'a'", "\"\u{01}\"", "tru", "nul",
            "{\"a\":1,\"a\":2}", "[1] 2", "\"\\ud800\"", "\"\\x\"", "1e400", "NaN",
        ]
        for text in rejected {
            XCTAssertThrowsError(try GeneratedContent(json: text), "accepted \(text.debugDescription)")
        }
    }

    func testDeepNestingIsRefused() {
        let deep = String(repeating: "[", count: 600) + String(repeating: "]", count: 600)
        XCTAssertThrowsError(try GeneratedContent(json: deep))
        let fine = String(repeating: "[", count: 100) + String(repeating: "]", count: 100)
        XCTAssertNoThrow(try GeneratedContent(json: fine))
    }

    func testErrorsSayWhere() {
        XCTAssertThrowsError(try GeneratedContent(json: #"{"a": tru}"#)) {
            XCTAssertEqual(($0 as? JSONParseError)?.offset, 9)
        }
    }

    func testNumbers() throws {
        XCTAssertEqual(try GeneratedContent(json: "42").intValue, 42)
        XCTAssertEqual(try GeneratedContent(json: "-3.25e1").doubleValue, -32.5)
        XCTAssertNil(try GeneratedContent(json: "4.5").intValue)
        XCTAssertEqual(GeneratedContent(kind: .number(3)).jsonString, "3")
    }
}
