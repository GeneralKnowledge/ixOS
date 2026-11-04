import Foundation
import WeatherBridge

/// Presentation state for the weather screen.
///
/// Holds no weather logic: fetching, decoding, cancellation and the offline
/// cache are all in Rust behind `WeatherClient`. Location comes from a
/// `LocationProviding` (CoreLocation in the app, a fake in tests).
@MainActor
public final class WeatherViewModel: ObservableObject {
    /// The report on screen (live or cached), or nil before the first one.
    @Published public private(set) var report: WeatherReport?
    /// The place the next refresh is for.
    @Published public private(set) var place: Place?
    @Published public private(set) var isRefreshing = false
    /// Why the last refresh failed, when there is no report to show instead.
    @Published public private(set) var errorMessage: String?
    /// Why "current location" did not work.
    @Published public private(set) var locationMessage: String?

    @Published public private(set) var searchResults: [Place] = []
    @Published public private(set) var isSearching = false
    @Published public private(set) var searchMessage: String?

    public let client: WeatherClient
    private let location: any LocationProviding
    private var refreshTask: Task<Void, Never>?
    private var refreshGeneration = 0
    private var searchTask: Task<Void, Never>?
    private var started = false

    /// - Parameters:
    ///   - client: defaults to a client caching under Application Support.
    ///   - location: defaults to CoreLocation.
    public init(client: WeatherClient? = nil, location: (any LocationProviding)? = nil) {
        self.client = client ?? WeatherClient(storageDirectory: Self.defaultStorageDirectory())
        self.location = location ?? DeviceLocationProvider()
    }

    /// Shows the cached report at once, then refreshes it, or asks for the
    /// device location on first launch. Call from `.task`; later calls do
    /// nothing.
    public func start() async {
        guard !started else { return }
        started = true
        if let cached = client.cachedReport {
            report = cached
            place = cached.place
            await refresh().value
        } else {
            await useCurrentLocation().value
        }
    }

    /// Gets a location fix and refreshes for it.
    @discardableResult
    public func useCurrentLocation() -> Task<Void, Never> {
        locationMessage = nil
        isRefreshing = true
        let location = self.location
        return Task {
            do {
                let fix = try await location.currentLocation()
                await select(.currentLocation(fix)).value
            } catch let error as LocationError {
                locationMessage = error.userMessage
                if refreshTask == nil { isRefreshing = false }
            } catch {
                locationMessage = LocationError.unavailable(detail: "\(error)").userMessage
                if refreshTask == nil { isRefreshing = false }
            }
        }
    }

    /// Switches to `place` and refreshes.
    @discardableResult
    public func select(_ place: Place) -> Task<Void, Never> {
        self.place = place
        searchResults = []
        searchMessage = nil
        return refresh()
    }

    /// Refreshes the current place. A refresh already running is cancelled
    /// (Rust would cancel it anyway; this keeps the Swift task tidy).
    @discardableResult
    public func refresh() -> Task<Void, Never> {
        guard let place else {
            return Task {}
        }
        refreshTask?.cancel()
        refreshGeneration += 1
        let generation = refreshGeneration
        isRefreshing = true
        let client = self.client
        let task = Task {
            do {
                let report = try await client.report(for: place)
                self.report = report
                errorMessage = nil
            } catch is CancellationError {
                // Superseded or cancelled: whoever cancelled owns the UI now.
            } catch let error as WeatherClientError {
                errorMessage = error.userMessage
                // A report for another place would be misleading.
                if self.report.map({ !Self.samePlace($0.place, place) }) ?? false {
                    self.report = nil
                }
            } catch {
                errorMessage = WeatherClientError.internalFailure(detail: "\(error)").userMessage
            }
            if generation == refreshGeneration {
                isRefreshing = false
                refreshTask = nil
            }
        }
        refreshTask = task
        return task
    }

    /// Cancels the refresh in flight. The screen keeps what it had.
    public func cancelRefresh() {
        refreshTask?.cancel()
        refreshTask = nil
        refreshGeneration += 1
        isRefreshing = false
    }

    /// Searches for places by name. A newer search replaces an older one.
    @discardableResult
    public func search(_ query: String) -> Task<Void, Never> {
        searchTask?.cancel()
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.count >= 2 else {
            searchResults = []
            searchMessage = nil
            isSearching = false
            return Task {}
        }
        isSearching = true
        searchMessage = nil
        let client = self.client
        let task = Task {
            do {
                let places = try await client.searchPlaces(trimmed)
                searchResults = places
                searchMessage = places.isEmpty ? "No places match “\(trimmed)”." : nil
                isSearching = false
            } catch is CancellationError {
                // A newer search is running.
            } catch let error as WeatherClientError {
                searchResults = []
                searchMessage = error.userMessage
                isSearching = false
            } catch {
                searchResults = []
                searchMessage = "\(error)"
                isSearching = false
            }
        }
        searchTask = task
        return task
    }

    /// "Showing weather from 3 h ago: you're offline.", or nil for live data.
    public var staleNotice: String? {
        guard let report, case let .cached(ageSeconds, problem) = report.freshness else {
            return nil
        }
        let age = WeatherFormat.age(seconds: ageSeconds)
        switch problem {
        case .none: return "Updated \(age) ago."
        case .offline: return "Showing weather from \(age) ago: you're offline."
        case .timedOut: return "Showing weather from \(age) ago: the service didn't answer."
        case .serverError: return "Showing weather from \(age) ago: the service is having problems."
        case .badResponse: return "Showing weather from \(age) ago: the service sent bad data."
        }
    }

    /// Stops everything. The cache is already on disk.
    public func close() {
        refreshTask?.cancel()
        searchTask?.cancel()
        client.shutdown()
    }

    private static func samePlace(_ a: Place, _ b: Place) -> Bool {
        abs(a.coordinate.latitude - b.coordinate.latitude) < 0.01
            && abs(a.coordinate.longitude - b.coordinate.longitude) < 0.01
    }

    private static func defaultStorageDirectory() -> String {
        let fm = FileManager.default
        let base = (try? fm.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true))
            ?? fm.temporaryDirectory
        let dir = base.appendingPathComponent("Weather", isDirectory: true)
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.path
    }
}
