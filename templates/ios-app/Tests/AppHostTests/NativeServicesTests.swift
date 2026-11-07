import AppHost
@preconcurrency import ForgeFFI
import Foundation
import XCTest

/// The native services' Swift adapters (copied into ForgeFFI by
/// `forge build`) against the app's real Rust core. Runs under `swift test`,
/// which is not an app bundle, so the notification centre is absent and
/// those services report `Unavailable` / `.unsupported`; the device runbook
/// covers them on an iPhone.
final class NativeServicesTests: XCTestCase {
    func testInstallReachesRust() {
        XCTAssertEqual(nativeServicesVersion(), 3)
        XCTAssertTrue(ForgeServices.install(keychainService: "forge-tests-\(UUID().uuidString)"))
        XCTAssertTrue(nativeServicesInstalled())
    }

    func testDirectoriesAreAbsoluteAndExist() throws {
        let dirs = try ForgeFiles.appDirectories()
        for path in [dirs.documents, dirs.applicationSupport, dirs.caches, dirs.temporary] {
            XCTAssertTrue(path.hasPrefix("/"), path)
            var isDirectory: ObjCBool = false
            XCTAssertTrue(FileManager.default.fileExists(atPath: path, isDirectory: &isDirectory), path)
            XCTAssertTrue(isDirectory.boolValue, path)
        }
    }

    func testKeychainRoundTrip() throws {
        let store = KeychainSecureStore(service: "forge-tests-\(UUID().uuidString)")
        do {
            XCTAssertNil(try store.get(key: "token"))
            try store.set(key: "token", value: Data("one".utf8))
            try store.set(key: "token", value: Data("two".utf8))  // replaces
            XCTAssertEqual(try store.get(key: "token"), Data("two".utf8))
            try store.delete(key: "token")
            try store.delete(key: "token")  // deleting a missing key is fine
            XCTAssertNil(try store.get(key: "token"))
        } catch NativeServiceError.Platform(let code, let reason)
            where [-34018, -25308, -25291, -25307].contains(code)
        {
            // No usable keychain in this process (missing entitlement, locked
            // or absent keychain on a CI machine).
            throw XCTSkip("keychain unavailable here: \(reason)")
        }
    }

    func testFileImportMakesAnOwnedCopy() throws {
        let source = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-import-\(UUID().uuidString).txt")
        try Data("hello".utf8).write(to: source)
        let inbox = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-inbox-\(UUID().uuidString)", isDirectory: true)
        addTeardownBlock {
            try? FileManager.default.removeItem(at: source)
            try? FileManager.default.removeItem(at: inbox)
        }
        let first = try ForgeFiles.importCopy(of: source, into: inbox)
        let second = try ForgeFiles.importCopy(of: source, into: inbox)
        XCTAssertNotEqual(first, second)
        XCTAssertEqual(try Data(contentsOf: first), Data("hello".utf8))
        XCTAssertEqual(try Data(contentsOf: second), Data("hello".utf8))
    }

    func testNotificationsReportUnavailableOutsideAnApp() async {
        XCTAssertFalse(ForgeServices.isAppBundle)
        let notifier = ForgeNotifications.makeNotifier()
        do {
            try await notifier.schedule(
                notification: LocalNotification(
                    id: "n", title: "T", body: "B", subtitle: nil, threadId: nil, deepLink: nil,
                    trigger: .now))
            XCTFail("scheduling should fail outside an app bundle")
        } catch NativeServiceError.Unavailable {
        } catch {
            XCTFail("unexpected error \(error)")
        }
        let status = await ForgePermissions().status(permission: .notifications)
        XCTAssertEqual(status, .unsupported)
    }

    @MainActor
    func testDeepLinksWaitForAHandlerThenArriveParsed() throws {
        let links = ForgeDeepLinks()
        XCTAssertTrue(links.open(try XCTUnwrap(URL(string: "myapp://note/42?from=test"))))
        XCTAssertFalse(links.open(try XCTUnwrap(URL(string: "no-scheme"))))

        var received: [DeepLink] = []
        let token = links.addHandler { received.append($0) }
        XCTAssertEqual(received.map(\.route), [["note", "42"]])
        XCTAssertEqual(received.first?.query, [QueryItem(name: "from", value: "test")])

        links.open(try XCTUnwrap(URL(string: "https://example.com/note/7")))
        XCTAssertEqual(received.map(\.route), [["note", "42"], ["note", "7"]])
        XCTAssertEqual(links.latest?.host, "example.com")

        links.removeHandler(token)
        links.open(try XCTUnwrap(URL(string: "myapp://later")))
        XCTAssertEqual(received.count, 2)
    }
}
