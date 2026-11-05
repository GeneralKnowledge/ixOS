import Foundation
import XCTest

/// Runs `scripts/mock-chat-server` for a test, on a free port.
///
/// `scripts/test-swift-host.sh` builds the server and sets
/// `CHAT_MOCK_SERVER` to its path. Without it (a plain `swift test`), tests
/// that need the server are skipped; CI always sets it.
final class MockServer {
    let url: String
    private let process: Process

    static func start(heartbeatMs: Int = 200) throws -> MockServer {
        guard let path = ProcessInfo.processInfo.environment["CHAT_MOCK_SERVER"], !path.isEmpty
        else {
            throw XCTSkip("CHAT_MOCK_SERVER is not set; run scripts/test-swift-host.sh examples/chat")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = ["--bind", "127.0.0.1:0", "--heartbeat-ms", String(heartbeatMs)]
        let output = Pipe()
        process.standardOutput = output
        try process.run()

        // The server's first line is "mock chat server on http://127.0.0.1:PORT (...".
        var line = ""
        while !line.contains("\n") {
            let data = output.fileHandleForReading.availableData
            if data.isEmpty { break }
            line += String(decoding: data, as: UTF8.self)
        }
        guard let start = line.range(of: "http://"),
            let end = line[start.lowerBound...].firstIndex(of: " ")
        else {
            process.terminate()
            throw NSError(
                domain: "MockServer", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "unexpected server output: \(line)"])
        }
        return MockServer(url: String(line[start.lowerBound..<end]), process: process)
    }

    private init(url: String, process: Process) {
        self.url = url
        self.process = process
    }

    /// Kills the server, like Ctrl-C, and waits for it to exit.
    func stop() {
        guard process.isRunning else { return }
        process.terminate()
        process.waitUntilExit()
    }

    deinit {
        stop()
    }
}

/// Polls `condition` on the main actor until it holds or `timeout` passes.
@MainActor
func waitUntil(
    _ what: String, timeout: TimeInterval = 10, file: StaticString = #filePath, line: UInt = #line,
    _ condition: () -> Bool
) async {
    let deadline = Date().addingTimeInterval(timeout)
    while !condition() {
        if Date() > deadline {
            XCTFail("timed out waiting until \(what)", file: file, line: line)
            return
        }
        try? await Task.sleep(nanoseconds: 20_000_000)
    }
}

func temporaryDirectory(_ test: XCTestCase) throws -> String {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("forge-chat-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    test.addTeardownBlock { try? FileManager.default.removeItem(at: url) }
    return url.path
}
