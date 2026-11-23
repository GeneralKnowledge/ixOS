import AppHost
import ForgeFFI
import Foundation
import XCTest

/// Python Lab's live console (python-lab-workspace-plan.md, PL1d): output
/// as it comes, stderr apart, answers to input(), Stop while waiting, the
/// time limit's note and the trimming notice. A fake runtime stands in for
/// the interpreter.
@MainActor
final class PythonConsoleTests: XCTestCase {
    /// Plays a script of console events; `readLine` waits for the model's
    /// answer the way PythonBridge's input() does.
    private final class FakeRuntime: PythonRunning, @unchecked Sendable {
        typealias Events = @Sendable (PythonConsoleEvent) -> Void
        let script: @Sendable (FakeRuntime, Events) async -> PythonRunResult
        private let lock = NSLock()
        private var answer: CheckedContinuation<String?, Never>?

        init(script: @escaping @Sendable (FakeRuntime, Events) async -> PythonRunResult) {
            self.script = script
        }

        func start() async throws -> String { "3.15.0 (fake)" }

        func run(
            _ code: String, id: UUID, limit: Duration, events: @escaping Events
        ) async -> PythonRunResult {
            await script(self, events)
        }

        func send(input line: String, id: UUID) -> Bool {
            lock.withLock {
                guard let answer else { return false }
                self.answer = nil
                answer.resume(returning: line)
                return true
            }
        }

        func interrupt(id: UUID) {
            lock.withLock {
                answer?.resume(returning: nil)
                answer = nil
            }
        }

        func readLine(_ events: Events) async -> String? {
            let line: String? = await withCheckedContinuation { continuation in
                lock.withLock { answer = continuation }
                events(.waitingForInput)
            }
            events(.inputEnded(line))
            return line
        }
    }

    private func model(
        _ runtime: FakeRuntime, activity: PythonActivity = PythonActivity()
    ) -> PythonLabModel {
        PythonLabModel(
            runtime: runtime,
            defaults: UserDefaults(suiteName: "PythonConsoleTests.\(UUID())")!,
            activity: activity)
    }

    private func waitUntil(
        _ what: String, _ condition: () -> Bool
    ) async throws {
        for _ in 0..<300 {
            if condition() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTFail("Timed out waiting until \(what)")
    }

    func testOutputShowsWhileTheScriptRuns() async throws {
        let runtime = FakeRuntime { _, events in
            events(.started)
            events(.output("first\n"))
            try? await Task.sleep(for: .milliseconds(400))
            events(.output("second\n"))
            return PythonRunResult(ok: true, output: "")
        }
        let lab = model(runtime)
        let run = Task { await lab.run() }
        try await waitUntil("the first line shows") { lab.output.contains("first") }
        XCTAssertTrue(lab.isRunning)
        XCTAssertFalse(lab.output.contains("second"))
        await run.value
        XCTAssertEqual(lab.output, "first\nsecond\n")
        XCTAssertEqual(lab.succeeded, true)
    }

    func testStderrIsKeptApart() async {
        let runtime = FakeRuntime { _, events in
            events(.output("ok\n"))
            events(.error("Traceback (most recent call last):\nZeroDivisionError\n"))
            return PythonRunResult(ok: false, output: "")
        }
        let lab = model(runtime)
        await lab.run()
        XCTAssertEqual(lab.console.segments.map(\.kind), [.output, .error])
        XCTAssertEqual(lab.succeeded, false)
    }

    func testInputWaitsForTheAnswer() async throws {
        let activity = PythonActivity()
        let runtime = FakeRuntime { fake, events in
            events(.output("Name? "))
            let name = await fake.readLine(events) ?? "?"
            events(.output("Hello \(name)\n"))
            return PythonRunResult(ok: true, output: "")
        }
        let lab = model(runtime, activity: activity)
        XCTAssertFalse(lab.sendInput("too early"))
        let run = Task { await lab.run() }
        try await waitUntil("input() waits") { lab.waitingForInput }
        XCTAssertTrue(activity.labWaitingForInput)
        XCTAssertTrue(lab.sendInput("Ada\nignored"))
        await run.value
        XCTAssertEqual(lab.output, "Name? Ada\nHello Ada\n")
        XCTAssertEqual(lab.console.segments.map(\.kind), [.output, .input, .output])
        XCTAssertFalse(lab.waitingForInput)
        XCTAssertFalse(activity.labWaitingForInput)
    }

    func testStopWakesAWaitingInput() async throws {
        let runtime = FakeRuntime { fake, events in
            if await fake.readLine(events) == nil {
                events(.error("KeyboardInterrupt\n"))
                return PythonRunResult(ok: false, output: "")
            }
            return PythonRunResult(ok: true, output: "")
        }
        let lab = model(runtime)
        let run = Task { await lab.run() }
        try await waitUntil("input() waits") { lab.waitingForInput }
        lab.stop()
        await run.value
        XCTAssertEqual(lab.output, "KeyboardInterrupt\n")
        XCTAssertEqual(lab.succeeded, false)
        XCTAssertFalse(lab.waitingForInput)
    }

    func testDebugPausesTakesCommandsAndKeepsThemOutOfTheConsole() async throws {
        let pause = #"{"stack": [{"file": "main.py", "line": 1, "function": "<module>"}], "variables": [{"name": "x", "kind": "int", "value": "1"}], "more": false}"#
        let runtime = FakeRuntime { fake, events in
            events(.debugPaused(pause))
            let command = await fake.readLine(events)
            events(.output("after \(command ?? "stop")\n"))
            return PythonRunResult(ok: true, output: "")
        }
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("debug-\(UUID().uuidString)")
        let lab = PythonLabModel(
            runtime: runtime, workspace: try Workspace(projectsFolder: folder.path),
            defaults: UserDefaults(suiteName: "PythonConsoleTests.\(UUID())")!,
            activity: PythonActivity())
        lab.toggleBreakpoint(line: 1)
        XCTAssertEqual(lab.breakpointLines, [1])
        XCTAssertFalse(lab.debug(.next))
        let run = Task { await lab.run(debugging: true) }
        try await waitUntil("the debugger waits") { lab.awaitingDebugCommand }
        XCTAssertTrue(lab.isDebugging)
        XCTAssertEqual(lab.pausedLine, 1)
        XCTAssertEqual(lab.debugPause?.variables.first?.value, "1")
        XCTAssertFalse(lab.waitingForInput)
        XCTAssertTrue(lab.debug(.next))
        await run.value
        XCTAssertEqual(lab.output, "after next\n")
        XCTAssertFalse(lab.isDebugging)
        XCTAssertNil(lab.debugPause)
        lab.toggleBreakpoint(line: 1)
        XCTAssertTrue(lab.breakpoints.isEmpty)
    }

    func testTheTimeLimitIsNoted() async {
        let runtime = FakeRuntime { _, events in
            events(.output("looping\n"))
            return PythonRunResult(ok: false, output: "", timedOut: true)
        }
        let lab = model(runtime)
        await lab.run()
        XCTAssertTrue(lab.output.hasPrefix("looping\n"))
        XCTAssertTrue(lab.output.contains("Reached the 30 s limit"))
        XCTAssertEqual(lab.console.segments.last?.kind, .note)
    }

    func testClearEmptiesTheConsole() async {
        let runtime = FakeRuntime { _, events in
            events(.output("x\n"))
            return PythonRunResult(ok: true, output: "")
        }
        let lab = model(runtime)
        await lab.run()
        lab.clearConsole()
        XCTAssertTrue(lab.console.isEmpty)
        XCTAssertNil(lab.succeeded)
    }

    func testLongOutputKeepsItsEndAndSaysWhatWentMissing() {
        var console = PythonConsoleText(limit: 10)
        console.append("12345\n67890\n", as: .output)
        console.append("abc", as: .error)
        // 15 bytes over a 10-byte limit: the first whole line goes.
        XCTAssertEqual(console.segments.map(\.text), ["67890\n", "abc"])
        XCTAssertEqual(console.removedBytes, 6)
        XCTAssertEqual(console.removedNotice, "Earlier output removed (1 KB)")
        XCTAssertEqual(console.plainText, "Earlier output removed (1 KB)\n67890\nabc")
    }

    func testTrimmingNeverSplitsACharacter() {
        var console = PythonConsoleText(limit: 4)
        console.append("ééé", as: .output)  // 6 bytes, no newline
        XCTAssertEqual(console.segments.map(\.text), ["éé"])
        XCTAssertEqual(console.removedBytes, 2)
    }
}
