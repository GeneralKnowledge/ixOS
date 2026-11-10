// The showcase's models import only Foundation, Observation and ForgeUI's
// tokens, so these run on Linux with `swift test`.
import ForgeUI
import ForgeUIShowcase
import XCTest

@MainActor
final class ShowcaseModelTests: XCTestCase {
    func testShowcaseThemesAreReadable() {
        for choice in ShowcaseTheme.allCases {
            let issues = choice.theme.contrastIssues()
            XCTAssertTrue(issues.isEmpty, "\(choice): " + issues.map(\.description).joined(separator: "; "))
        }
    }

    func testTitleRulesBecomeFieldMessages() {
        XCTAssertNil(TaskRules.titleMessage(TaskDraft(title: ""), touched: false), "no error before the person types")
        XCTAssertEqual(TaskRules.titleMessage(TaskDraft(title: "  "), touched: true), .error("Give the task a title."))
        XCTAssertEqual(TaskRules.titleMessage(TaskDraft(title: String(repeating: "a", count: 70)), touched: true)?.kind, .warning)
        XCTAssertEqual(TaskRules.titleMessage(TaskDraft(title: String(repeating: "a", count: 81)), touched: true)?.kind, .error)
        XCTAssertTrue(TaskRules.canSave(TaskDraft(title: String(repeating: "a", count: 70))), "a warning doesn't block saving")
        XCTAssertFalse(TaskRules.canSave(TaskDraft(title: "Fine", notes: String(repeating: "n", count: 501))))
    }

    func testFormSavesNewAndEditedTasks() async {
        let tasks = TasksModel(tasks: [])
        let form = TaskFormModel()
        XCTAssertNil(form.titleMessage)
        let refused = await form.save(into: tasks)
        XCTAssertFalse(refused)
        XCTAssertEqual(form.titleMessage?.kind, .error, "trying to save shows the error")

        form.title = "  Water the plants "
        form.priority = .high
        let saved = await form.save(into: tasks)
        XCTAssertTrue(saved)
        XCTAssertFalse(form.isSaving)
        XCTAssertEqual(tasks.tasks.map(\.title), ["Water the plants"])

        let edit = TaskFormModel(editing: tasks.tasks[0])
        XCTAssertFalse(edit.isNew)
        edit.title = "Water the garden"
        _ = await edit.save(into: tasks)
        XCTAssertEqual(tasks.tasks.map(\.title), ["Water the garden"])
        XCTAssertEqual(tasks.tasks[0].priority, .high)
    }

    func testSearchAndOrder() {
        let tasks = TasksModel()
        XCTAssertEqual(tasks.visibleTasks.last?.isDone, true, "done tasks sort last")
        tasks.searchText = "PASSPORT"
        XCTAssertEqual(tasks.visibleTasks.map(\.title), ["Renew passport"])
        tasks.searchText = "drawer"
        XCTAssertEqual(tasks.visibleTasks.count, 1, "notes are searched too")
        tasks.searchText = "nothing like this"
        XCTAssertTrue(tasks.visibleTasks.isEmpty)
    }

    func testLoadStates() async {
        let tasks = TasksModel()
        await tasks.reload(failing: true)
        guard case .failed = tasks.phase else { return XCTFail("expected failed, got \(tasks.phase)") }
        await tasks.reload(failing: false)
        XCTAssertEqual(tasks.phase, .loaded)
        tasks.deleteAll()
        XCTAssertTrue(tasks.tasks.isEmpty)
        tasks.restoreSamples()
        XCTAssertEqual(tasks.tasks.count, ShowcaseTask.samples.count)
    }
}
