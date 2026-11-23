import FORGE_MODULE_NAMEApp
import XCTest

/// What a note's editor does with a draft an earlier save failed to keep.
final class NoteEditorTextTests: XCTestCase {
    func testItOpensOnTheDraftAndDropsItWhenBackToTheSavedText() {
        var editor = NoteEditorText(saved: "saved", retained: "draft A")
        XCTAssertEqual(editor.text, "draft A")
        editor.text = "saved"
        XCTAssertEqual(editor.leaving(saved: "saved", retained: "draft A"), .discardDraft)
    }

    func testALateFailureIsShownWhenTheTextIsUntouched() {
        // Opened on the saved text while a save was running; it then failed.
        var editor = NoteEditorText(saved: "saved", retained: nil)
        editor.retainedChanged("draft B", saved: "saved")
        XCTAssertEqual(editor.text, "draft B")
        XCTAssertEqual(editor.leaving(saved: "saved", retained: "draft B"), .save("draft B"))
    }

    func testUseSavedVersionThenALateFailureKeepsTheLaterDraft() {
        var editor = NoteEditorText(saved: "saved", retained: "draft A")
        editor.useSaved("saved")
        XCTAssertEqual(editor.text, "saved")
        // Draft B's save fails after Use Saved Version.
        editor.retainedChanged("draft B", saved: "saved")
        XCTAssertEqual(editor.text, "draft B")
        XCTAssertEqual(editor.leaving(saved: "saved", retained: "draft B"), .save("draft B"))
    }

    func testAnUnchangedLoadedDraftGivesWayToALaterFailure() {
        var editor = NoteEditorText(saved: "saved", retained: "draft A")
        editor.retainedChanged("draft B", saved: "saved")
        XCTAssertEqual(editor.text, "draft B")
    }

    func testNewerEditsAreKeptOverALateFailure() {
        var editor = NoteEditorText(saved: "saved", retained: nil)
        editor.text = "typing now"
        editor.retainedChanged("draft B", saved: "saved")
        XCTAssertEqual(editor.text, "typing now")
        XCTAssertEqual(editor.leaving(saved: "saved", retained: "draft B"), .save("typing now"))
    }

    func testADraftThisEditorDidNotLoadIsNeverDropped() {
        var editor = NoteEditorText(saved: "saved", retained: nil)
        editor.text = "typing now"
        editor.text = "saved"
        XCTAssertEqual(editor.leaving(saved: "saved", retained: "draft B"), .nothing)
    }

    func testBlankTextIsNotSaved() {
        var editor = NoteEditorText(saved: "saved", retained: nil)
        editor.text = "  \n"
        XCTAssertEqual(editor.leaving(saved: "saved", retained: nil), .nothing)
    }
}
