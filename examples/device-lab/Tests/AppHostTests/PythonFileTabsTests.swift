import AppHost
import ForgeFFI
import Foundation
import XCTest

/// Python Lab's open-file tabs (PL2) on a real workspace in a temporary
/// folder.
@MainActor
final class PythonFileTabsTests: XCTestCase {
    private func model() throws -> PythonLabModel {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("tabs-\(UUID().uuidString)")
        let workspace = try Workspace(projectsFolder: folder.path)
        return PythonLabModel(
            runtime: nil, workspace: workspace,
            defaults: UserDefaults(suiteName: "PythonFileTabsTests.\(UUID())")!,
            activity: PythonActivity())
    }

    func testOpeningFilesAddsTabsAndClosingSwitches() throws {
        let lab = try model()
        XCTAssertEqual(lab.openFiles, ["main.py"])
        XCTAssertNil(lab.createFile("helper.py", text: "x = 1\n"))
        XCTAssertEqual(lab.openFiles, ["main.py", "helper.py"])
        XCTAssertEqual(lab.openFile, "helper.py")
        lab.close(file: "helper.py")
        XCTAssertEqual(lab.openFiles, ["main.py"])
        XCTAssertEqual(lab.openFile, "main.py")
        // The last tab stays.
        lab.close(file: "main.py")
        XCTAssertEqual(lab.openFiles, ["main.py"])
    }

    func testTabsComeBackWithTheProject() throws {
        let lab = try model()
        XCTAssertNil(lab.createFile("helper.py"))
        XCTAssertNil(lab.createProject("other"))
        lab.open(project: "other")
        XCTAssertFalse(lab.openFiles.contains("helper.py"))
        lab.open(project: "scratch")
        XCTAssertEqual(lab.openFiles, ["main.py", "helper.py"])
        XCTAssertEqual(lab.openFile, "helper.py")
    }

    func testTooManyTabsClosesTheOldest() throws {
        let lab = try model()
        for n in 1...PythonLabModel.maxOpenFiles {
            XCTAssertNil(lab.createFile("f\(n).py"))
        }
        XCTAssertEqual(lab.openFiles.count, PythonLabModel.maxOpenFiles)
        XCTAssertFalse(lab.openFiles.contains("main.py"))
        XCTAssertEqual(lab.openFile, "f\(PythonLabModel.maxOpenFiles).py")
    }

    func testRenamingAndDeletingFollowTheTabs() throws {
        let lab = try model()
        XCTAssertNil(lab.createFile("helper.py"))
        let entry = try XCTUnwrap(lab.entries.first { $0.path == "helper.py" })
        XCTAssertNil(lab.rename(entry, to: "tools.py"))
        XCTAssertEqual(lab.openFiles, ["main.py", "tools.py"])
        let renamed = try XCTUnwrap(lab.entries.first { $0.path == "tools.py" })
        lab.delete(renamed)
        XCTAssertEqual(lab.openFiles, ["main.py"])
        XCTAssertEqual(lab.openFile, "main.py")
    }
}
