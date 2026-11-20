import ForgeKit
import Foundation
@testable import WeatherApp
@preconcurrency import WeatherFFI
import XCTest

private struct FakeLocation: LocationProviding {
    let result: Result<LocationReading, LocationError>

    func currentLocation() async throws -> LocationReading {
        try result.get()
    }
}

/// Answers each lookup with the next reading, repeating the last.
private final class MovingLocation: LocationProviding, @unchecked Sendable {
    private let lock = NSLock()
    private var readings: [LocationReading]
    private var _lookups = 0
    var lookups: Int { lock.withLock { _lookups } }

    init(_ readings: [LocationReading]) { self.readings = readings }

    func currentLocation() async throws -> LocationReading {
        lock.withLock {
            _lookups += 1
            return readings.count > 1 ? readings.removeFirst() : readings[0]
        }
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
                airQualityBaseUrl: "http://127.0.0.1:9", requestTimeoutMs: 5_000))
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
        let fix = LocationReading(
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
        let fix = LocationReading(
            coordinate: Coordinate(latitude: 55.9, longitude: -3.64), placeName: "Bathgate")
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .success(fix)))
        await model.start()
        XCTAssertEqual(model.place?.name, "Bathgate")
        XCTAssertTrue(model.selectedPage?.isMyLocation ?? false)
    }

    @MainActor
    func testALoadFailureShowsAMessageAndTryAgainLoads() async throws {
        let server = try FixtureServer(body: parisForecastJSON)
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-weather-vm-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        let base = "http://127.0.0.1:\(server.port)"
        let client = try WeatherClient(
            storageDirectory: url.path,
            configuration: WeatherConfig(
                forecastBaseUrl: base, geocodingBaseUrl: base, airQualityBaseUrl: base, requestTimeoutMs: 5_000))
        let model = WeatherViewModel(
            client: client, location: FakeLocation(result: .failure(.denied)))
        await model.start()
        let paris = Place(
            name: "Paris", region: nil, country: nil, countryCode: nil,
            coordinate: Coordinate(latitude: 48.86, longitude: 2.34), timezone: nil,
            population: nil, source: .search(geocodingId: nil))
        model.add(paris)
        let id = model.selection
        await model.refresh(id).value
        XCTAssertNil(model.report, "the service is down")
        XCTAssertNotNil(model.errorMessage)
        XCTAssertFalse(model.isRefreshing)

        // Try Again, with the service back.
        server.failing = false
        await model.refresh(id).value
        XCTAssertNotNil(model.report)
        XCTAssertNil(model.errorMessage)
        XCTAssertFalse(model.isRefreshing)
        XCTAssertNotNil(model.selectedPage?.overview)
        // The fixture answers the air-quality API with a forecast: it has
        // no pollen or air quality there, which the page says rather than
        // showing zero, and the forecast is unaffected.
        XCTAssertEqual(model.selectedPage?.overview?.pollen.state, .notAvailableHere)
        XCTAssertEqual(model.selectedPage?.overview?.airQuality.state, .notAvailableHere)
        XCTAssertEqual(model.selectedPage?.overview?.credits.first?.id, "open-meteo")
        model.close()
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
    func testUnreadableSavedPlacesAreKeptUntilThePersonStartsANewList() async throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-weather-vm-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        let file = url.appendingPathComponent("forge-weather-saved-places.json")
        try Data("{ damaged".utf8).write(to: file)
        let client = try WeatherClient(
            storageDirectory: url.path,
            configuration: WeatherConfig(
                forecastBaseUrl: "http://127.0.0.1:9", geocodingBaseUrl: "http://127.0.0.1:9",
                airQualityBaseUrl: "http://127.0.0.1:9", requestTimeoutMs: 5_000))
        let model = WeatherViewModel(client: client, location: FakeLocation(result: .failure(.denied)))
        await model.start()
        XCTAssertEqual(model.placesProblem, .unreadable)
        XCTAssertThrowsError(
            try client.save(
                Place(
                    name: "Prague", region: nil, country: nil, countryCode: nil,
                    coordinate: Coordinate(latitude: 50.09, longitude: 14.42), timezone: nil,
                    population: nil, source: .search(geocodingId: nil)))
        ) { XCTAssertEqual($0 as? WeatherClientError, .savedDataUnreadable) }
        XCTAssertEqual(try Data(contentsOf: file), Data("{ damaged".utf8), "never written over")

        model.startNewSavedPlaces()
        XCTAssertNil(model.placesProblem)
        XCTAssertNil(model.placesError)
        model.close()
    }

    @MainActor
    func testMyLocationIsFoundAgainAfterAWhileAway() async throws {
        let here = LocationReading(
            coordinate: Coordinate(latitude: 55.9, longitude: -3.64), placeName: "Bathgate")
        let there = LocationReading(
            coordinate: Coordinate(latitude: 48.86, longitude: 2.34), placeName: "Paris")
        let location = MovingLocation([here, there])
        let model = WeatherViewModel(client: try offlineClient(), location: location)
        var clock = Date()
        model.now = { clock }
        await model.start()
        XCTAssertEqual(model.place?.name, "Bathgate")

        // Back within ten minutes: the fix is still good.
        clock += 5 * 60
        model.becameActive()
        XCTAssertEqual(location.lookups, 1)
        // That refreshed the forecast instead (offline here); let it end.
        for _ in 0..<500 where model.selectedPage?.isRefreshing == true {
            try await Task.sleep(for: .milliseconds(10))
        }

        // Back after a long time away, through the host as on the phone:
        // the phone may have moved.
        let host = ForgeHost<ForgeWeatherApp>(environment: { try ForgeEnvironment.temporary() }) {
            try ForgeWeatherApp(storageDirectory: $0.dataDirectory)
        }
        model.follow(host)
        host.report(.background)
        clock += 60 * 60
        host.report(.active)
        for _ in 0..<200 where model.place?.name != "Paris" {
            try await Task.sleep(for: .milliseconds(10))
        }
        XCTAssertEqual(location.lookups, 2)
        XCTAssertEqual(model.place?.name, "Paris")
        model.close()
    }

    @MainActor
    func testALostLocationIsSaidAboveTheLastPlaceFound() async throws {
        let server = try FixtureServer(body: parisForecastJSON)
        server.failing = false
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("forge-weather-vm-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        let base = "http://127.0.0.1:\(server.port)"
        let config = WeatherConfig(forecastBaseUrl: base, geocodingBaseUrl: base, airQualityBaseUrl: base, requestTimeoutMs: 5_000)
        let paris = LocationReading(
            coordinate: Coordinate(latitude: 48.86, longitude: 2.34), placeName: "Paris")

        // One client for both: its storage stays open until the process
        // ends, as on the phone.
        let client = try WeatherClient(storageDirectory: url.path, configuration: config)

        // A first run finds the phone and keeps its forecast.
        let first = WeatherViewModel(client: client, location: FakeLocation(result: .success(paris)))
        await first.start()
        XCTAssertNotNil(first.report)
        XCTAssertNil(first.notice(for: try XCTUnwrap(first.selectedPage)))

        // The next run can't use location: the last place stays, and says why.
        let second = WeatherViewModel(client: client, location: FakeLocation(result: .failure(.denied)))
        await second.start()
        let page = try XCTUnwrap(second.selectedPage)
        XCTAssertTrue(page.isMyLocation)
        XCTAssertNotNil(page.report)
        XCTAssertEqual(
            second.notice(for: page),
            LocationError.denied.userMessage + " Showing the last place found.")
        second.close()
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
    func testAFailedAddSaysWhyAndKeepsTheSearchOpen() async throws {
        let client = try offlineClient()
        let model = WeatherViewModel(
            client: client, location: FakeLocation(result: .failure(.denied)))
        await model.start()
        let nowhere = Place(
            name: "Nowhere", region: nil, country: nil, countryCode: nil,
            coordinate: Coordinate(latitude: 200, longitude: 0), timezone: nil,
            population: nil, source: .search(geocodingId: nil))
        XCTAssertFalse(model.add(nowhere), "the sheet stays open")
        XCTAssertTrue(
            model.searchMessage?.hasPrefix("Couldn't add Nowhere.") ?? false,
            "\(model.searchMessage ?? "no message")")
        XCTAssertEqual(model.pages.map(\.id), [WeatherPage.myLocationID])
        XCTAssertTrue(client.savedPlaces.isEmpty)
        model.close()
    }

    @MainActor
    func testClearingStopsASearchStillRunning() async throws {
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .failure(.denied)))
        let running = model.search("Paris")
        XCTAssertTrue(model.isSearching)
        model.clearSearch()
        await running.value
        XCTAssertTrue(model.searchResults.isEmpty)
        XCTAssertNil(model.searchMessage, "the cleared search's answer isn't shown")
        XCTAssertFalse(model.isSearching)
    }

    @MainActor
    func testANewerSearchReplacesAnOlderOne() async throws {
        let model = WeatherViewModel(
            client: try offlineClient(), location: FakeLocation(result: .failure(.denied)))
        let older = model.search("Paris")
        let newer = model.search("Prague")
        await older.value
        XCTAssertTrue(model.isSearching, "the older answer doesn't end the newer search")
        await newer.value
        XCTAssertFalse(model.isSearching)
        XCTAssertEqual(model.searchMessage, WeatherClientError.offline(detail: "").userMessage)
    }

    @MainActor
    func testUnitsAreSavedAndUsedForEveryValue() throws {
        let client = try offlineClient()
        let model = WeatherViewModel(
            client: client, location: FakeLocation(result: .failure(.denied)),
            units: defaultUnits(regionCode: "FR"))
        XCTAssertEqual(model.format(.temperature(celsius: 0)), "0°")
        XCTAssertEqual(model.format(.speed(kmh: 36)), "36 km/h")
        model.setUnits(defaultUnits(regionCode: "US"))
        XCTAssertNil(model.unitsError)
        XCTAssertEqual(model.format(.temperature(celsius: 0)), "32°")
        XCTAssertEqual(model.format(.speed(kmh: 36)), "22 mph")
        XCTAssertEqual(model.format(.distance(metres: 24_140)), "15 mi")
        // Saved in the core: a new model on it starts in the chosen units.
        XCTAssertEqual(client.units(regionCode: "FR"), defaultUnits(regionCode: "US"))
        let next = WeatherViewModel(client: client, location: FakeLocation(result: .failure(.denied)))
        XCTAssertEqual(next.units, defaultUnits(regionCode: "US"))
    }

    @MainActor
    func testFormatting() {
        let metric = defaultUnits(regionCode: "FR")
        let us = defaultUnits(regionCode: "US")
        XCTAssertEqual(WeatherFormat.temperature(9.6, metric), "10°")
        XCTAssertEqual(WeatherFormat.temperature(-0.4, metric), "0°")
        XCTAssertEqual(WeatherFormat.temperature(0, us), "32°")
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
        XCTAssertEqual(WeatherFormat.speed(kmh: 32.2, units: us), "20 mph")
        XCTAssertEqual(WeatherFormat.speed(kmh: 32.2, units: metric), "32 km/h")
        XCTAssertEqual(WeatherFormat.quantity(.pressure(hpa: 1013.25), us), "29.92 inHg")
        XCTAssertEqual(WeatherFormat.problem(.timedOut), "the service took too long")
        XCTAssertEqual(WeatherFormat.moonName(.waxingGibbous), "Waxing gibbous")
        XCTAssertEqual(WeatherFormat.uvLevel(6.4), "High")
        XCTAssertTrue(WeatherFormat.isWet(.rainShowers(intensity: .light)))
        XCTAssertFalse(WeatherFormat.isWet(.overcast))
    }
}
