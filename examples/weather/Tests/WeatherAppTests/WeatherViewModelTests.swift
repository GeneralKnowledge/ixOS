import Foundation
import WeatherApp
import WeatherBridge
import XCTest

private struct FakeLocation: LocationProviding {
    let result: Result<LocationFix, LocationError>

    func currentLocation() async throws -> LocationFix {
        try result.get()
    }
}

final class WeatherViewModelTests: XCTestCase {
    @MainActor
    private func offlineClient() throws -> WeatherClient {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-weather-vm-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return try WeatherClient(
            storageDirectory: url.path,
            configuration: WeatherConfig(
                forecastBaseUrl: "http://127.0.0.1:9", geocodingBaseUrl: "http://127.0.0.1:9",
                requestTimeoutMs: 5_000))
    }

    @MainActor
    func testDeniedLocationExplainsAndStopsLoading() async throws {
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .failure(.denied)))
        await model.start()
        XCTAssertEqual(model.locationMessage, LocationError.denied.userMessage)
        XCTAssertFalse(model.isRefreshing)
        XCTAssertNil(model.place)
        XCTAssertNil(model.report)
    }

    @MainActor
    func testLocationFixRefreshesAndReportsOffline() async throws {
        let fix = LocationFix(
            coordinate: Coordinate(latitude: 51.5, longitude: -0.12), horizontalAccuracy: 65)
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .success(fix)))
        await model.start()
        XCTAssertEqual(model.place?.name, "Current location")
        XCTAssertEqual(model.place?.source, .device(horizontalAccuracyM: 65))
        XCTAssertTrue(model.place?.isDeviceLocation ?? false)
        XCTAssertNil(model.report)
        XCTAssertFalse(model.isRefreshing)
        XCTAssertEqual(
            model.errorMessage, WeatherClientError.offline(detail: "").userMessage)
    }

    @MainActor
    func testShortSearchClearsResultsWithoutARequest() async throws {
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .failure(.denied)))
        await model.search(" x ").value
        XCTAssertTrue(model.searchResults.isEmpty)
        XCTAssertNil(model.searchMessage)
        XCTAssertFalse(model.isSearching)
    }

    @MainActor
    func testOfflineSearchShowsAMessage() async throws {
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .failure(.denied)))
        await model.search("Paris").value
        XCTAssertFalse(model.isSearching)
        XCTAssertEqual(model.searchMessage, WeatherClientError.offline(detail: "").userMessage)
    }

    @MainActor
    func testFormatting() {
        XCTAssertEqual(WeatherFormat.temperature(9.6), "10°")
        XCTAssertEqual(WeatherFormat.temperature(-0.4), "0°")
        XCTAssertEqual(WeatherFormat.hour("2026-11-04T20:00"), "20:00")
        XCTAssertEqual(WeatherFormat.weekday("2026-11-04", isFirst: true), "Today")
        XCTAssertEqual(WeatherFormat.compass(315), "NW")
        XCTAssertEqual(WeatherFormat.compass(359), "N")
        XCTAssertEqual(WeatherFormat.age(seconds: 7_300), "2 h")
        XCTAssertEqual(
            WeatherFormat.describe(.thunderstorm(hail: .light)).text, "Thunderstorm with hail")
        XCTAssertEqual(
            WeatherFormat.describe(.rain(intensity: .heavy, freezing: false)).symbol,
            "cloud.heavyrain.fill")
        XCTAssertEqual(WeatherFormat.describe(.unknown(code: 4)).text, "Unknown (code 4)")
        XCTAssertEqual(WeatherFormat.describe(.clear, isDay: false).symbol, "moon.stars.fill")
    }
}
