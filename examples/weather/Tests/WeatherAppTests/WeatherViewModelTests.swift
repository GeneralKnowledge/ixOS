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
        XCTAssertEqual(model.pages.map(\.id), [WeatherPage.myLocationID])
        XCTAssertEqual(model.selection, WeatherPage.myLocationID)
    }

    @MainActor
    func testLocationFixRefreshesAndReportsOffline() async throws {
        let fix = LocationFix(
            coordinate: Coordinate(latitude: 51.5, longitude: -0.12), horizontalAccuracy: 65)
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .success(fix)))
        await model.start()
        XCTAssertEqual(model.place?.name, "My Location", "no locality from the fake provider")
        XCTAssertEqual(model.place?.source, .device(horizontalAccuracyM: 65))
        XCTAssertTrue(model.place?.isDeviceLocation ?? false)
        XCTAssertNil(model.report)
        XCTAssertFalse(model.isRefreshing)
        XCTAssertEqual(
            model.errorMessage, WeatherClientError.offline(detail: "").userMessage)
    }

    @MainActor
    func testTheLocalityNamesMyLocation() async throws {
        let fix = LocationFix(
            coordinate: Coordinate(latitude: 55.9, longitude: -3.64), placeName: "Bathgate")
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .success(fix)))
        await model.start()
        XCTAssertEqual(model.place?.name, "Bathgate")
        XCTAssertTrue(model.selectedPage?.isMyLocation ?? false)
    }

    @MainActor
    func testAddingRemovingAndReorderingPlaces() async throws {
        let client = try offlineClient()
        let model = WeatherViewModel(
            client: client, location: FakeLocation(result: .failure(.denied)))
        await model.start()
        func city(_ name: String, _ lat: Double, _ lon: Double) -> Place {
            Place(
                name: name, region: nil, country: nil, countryCode: nil,
                coordinate: Coordinate(latitude: lat, longitude: lon), timezone: nil,
                population: nil, source: .search(geocodingId: nil))
        }
        model.add(city("Düsseldorf", 51.22, 6.78))
        model.add(city("Prague", 50.09, 14.42))
        XCTAssertEqual(model.pages.map { $0.place?.name }, [nil, "Düsseldorf", "Prague"])
        XCTAssertEqual(model.place?.name, "Prague", "the new page is shown")
        XCTAssertEqual(client.savedPlaces.count, 2)

        model.add(city("Praha", 50.088, 14.421))
        XCTAssertEqual(model.pages.count, 3, "already saved")

        model.moveSaved(from: 1, to: 0)
        XCTAssertEqual(model.pages.map { $0.place?.name }, [nil, "Prague", "Düsseldorf"])

        model.remove([model.pages[1].id, WeatherPage.myLocationID])
        XCTAssertEqual(model.pages.map { $0.place?.name }, [nil, "Düsseldorf"], "My Location stays")
        XCTAssertEqual(client.savedPlaces.map(\.name), ["Düsseldorf"])
        XCTAssertNotNil(model.selectedPage)
        model.close()
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
        XCTAssertEqual(WeatherFormat.describe(.mainlyClear, isDay: true).text, "Mostly sunny")
        XCTAssertEqual(WeatherFormat.speed(kmh: 32.2, unit: .milesPerHour), "20 mph")
        XCTAssertEqual(WeatherFormat.speed(kmh: 32.2, unit: .kilometresPerHour), "32 km/h")
        XCTAssertEqual(WeatherFormat.uvLevel(6.4), "High")
        XCTAssertTrue(WeatherFormat.isWet(.rainShowers(intensity: .light)))
        XCTAssertFalse(WeatherFormat.isWet(.overcast))
    }
}
