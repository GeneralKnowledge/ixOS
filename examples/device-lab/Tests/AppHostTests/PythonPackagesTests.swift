import AppHost
import ForgeFFI
import Foundation
import XCTest

/// Python Lab's packages screen (PL2a) with a fake PyPI. Rust's own tests
/// (rust/src/packages/tests.rs) cover installing real wheels.
@MainActor
final class PythonPackagesTests: XCTestCase {
    private struct FakePyPI: PackageDownloading {
        var pages: [String: (String, Int)]

        func data(from url: URL) async throws -> (Data, Int) {
            let page = pages[url.absoluteString] ?? ("{\"message\": \"Not Found\"}", 404)
            return (Data(page.0.utf8), page.1)
        }

        func download(from url: URL) async throws -> URL {
            throw URLError(.cannotFindHost)
        }
    }

    private func model(_ pages: [String: (String, Int)] = [:]) -> (PythonPackagesModel, URL) {
        let site = FileManager.default.temporaryDirectory
            .appendingPathComponent("site-\(UUID().uuidString)")
        return (PythonPackagesModel(sitePackages: site.path, downloader: FakePyPI(pages: pages)), site)
    }

    func testAnUnknownPackageSaysSo() async {
        let (packages, _) = model()
        packages.name = "no-such-package"
        await packages.install()
        XCTAssertEqual(packages.error, "PyPI has no package called no-such-package.")
        XCTAssertFalse(packages.isWorking)
        XCTAssertEqual(packages.name, "no-such-package")
    }

    func testACompiledPackageIsExplained() async {
        let json = """
            {"info": {"name": "numpy", "version": "2.3.0"},
             "urls": [{"filename": "numpy-2.3.0-cp313-cp313-macosx_14_0_arm64.whl",
                       "packagetype": "bdist_wheel", "url": "https://files/n.whl",
                       "digests": {"sha256": "aa"}, "size": 10}]}
            """
        let (packages, _) = model(["https://pypi.org/pypi/numpy/json": (json, 200)])
        packages.name = "numpy"
        await packages.install()
        XCTAssertTrue(packages.error?.contains("compiled") ?? false, packages.error ?? "")
        XCTAssertTrue(packages.installed.isEmpty)
    }

    func testANameThatIsntOneIsRefusedBeforeAsking() async {
        let (packages, _) = model()
        packages.name = "../etc/passwd"
        await packages.install()
        XCTAssertTrue(packages.error?.contains("isn't a package name") ?? false)
    }
}
