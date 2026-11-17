import AppHost
import Foundation
import XCTest

/// The Checks screen: parsing the built-in list, answers kept per build,
/// and the shared text.
@MainActor
final class PhoneChecksModelTests: XCTestCase {
    private let text = """
    # Sitting
    ## Relay
    1. Send a folder.
    2) Receive photos.
    Some prose that isn't a check.
    ## Weather
    - My Location refreshes.
    """

    private func defaults() -> UserDefaults {
        let name = "checks-\(UUID().uuidString)"
        return UserDefaults(suiteName: name)!
    }

    func testParsesTitleGroupsAndItems() {
        let model = PhoneChecksModel(text: text, build: "1", defaults: defaults())
        XCTAssertEqual(model.title, "Sitting")
        XCTAssertEqual(model.groups.map(\.title), ["Relay", "Weather"])
        XCTAssertEqual(model.groups[0].items.map(\.text), ["Send a folder.", "Receive photos."])
        XCTAssertEqual(model.groups[1].items.map(\.text), ["My Location refreshes."])
    }

    func testAnswersLastForTheSameBuildOnly() {
        let store = defaults()
        let model = PhoneChecksModel(text: text, build: "1", defaults: store)
        let item = model.groups[0].items[0]
        model.set(.fail, for: item)
        model.setNote("froze", for: item)
        XCTAssertEqual(model.progress, "1 of 3 answered, 1 failed")

        let again = PhoneChecksModel(text: text, build: "1", defaults: store)
        XCTAssertEqual(again.answer(item), .init(result: .fail, note: "froze"))
        let nextBuild = PhoneChecksModel(text: text, build: "2", defaults: store)
        XCTAssertNil(nextBuild.answer(item).result)
    }

    func testTappingTheSameResultAgainClearsIt() {
        let model = PhoneChecksModel(text: text, build: "1", defaults: defaults())
        let item = model.groups[0].items[0]
        model.set(.pass, for: item)
        model.set(.pass, for: item)
        XCTAssertNil(model.answer(item).result)
    }

    func testShareTextListsEveryCheckWithItsMarkAndNote() {
        let model = PhoneChecksModel(text: text, build: "7", defaults: defaults())
        model.set(.pass, for: model.groups[0].items[0])
        model.set(.fail, for: model.groups[0].items[1])
        model.setNote("nothing arrived", for: model.groups[0].items[1])
        XCTAssertEqual(model.shareText, """
        Sitting, build 7

        Relay
        PASS Send a folder.
        FAIL Receive photos.
             Note: nothing arrived

        Weather
        ---- My Location refreshes.

        """)
    }
}
