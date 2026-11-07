import Foundation
import WeatherBridge

/// One page of the pager: "My Location", then each saved place.
public struct WeatherPage: Identifiable, Equatable, Sendable {
    public static let myLocationID = "my-location"

    public let id: String
    /// The place the next refresh is for. Nil for "My Location" until the
    /// first fix.
    public internal(set) var place: Place?
    /// The report on screen (live or cached), or nil before the first one.
    public internal(set) var report: WeatherReport?
    /// `report` as the screen shows it at the moment it was last worked out.
    public internal(set) var overview: WeatherOverview?
    public internal(set) var isRefreshing = false
    /// Why the last refresh failed. Shown above the report, or instead of it.
    public internal(set) var errorMessage: String?

    public var isMyLocation: Bool { id == Self.myLocationID }

    static func saved(_ place: Place) -> WeatherPage {
        WeatherPage(id: pageID(for: place), place: place)
    }

    static func pageID(for place: Place) -> String {
        String(format: "%.3f,%.3f", place.coordinate.latitude, place.coordinate.longitude)
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
}

/// Presentation state for the weather pager.
///
/// Holds no weather logic: fetching, decoding, cancellation, the offline
/// cache, saved places and what each page shows (`WeatherOverview`) are all
/// in Rust behind `WeatherClient`. Location comes from a `LocationProviding`
/// (CoreLocation in the app, a fake in tests).
@MainActor
public final class WeatherViewModel: ObservableObject {
    /// "My Location" first, then the saved places in the user's order.
    @Published public private(set) var pages: [WeatherPage]
    /// The page on screen.
    @Published public var selection: String = WeatherPage.myLocationID
    /// Why "My Location" did not work.
    @Published public private(set) var locationMessage: String?

    @Published public private(set) var searchResults: [Place] = []
    @Published public private(set) var isSearching = false
    @Published public private(set) var searchMessage: String?

    public let client: WeatherClient
    /// How the summary and the wind tile state wind speed.
    public let windUnit: WindUnit
    private let location: any LocationProviding
    /// False shows only what is cached: no location request, no network.
    /// The simulator screenshot job uses it (launch argument
    /// `-WeatherOfflineDemo YES`).
    private let isLive: Bool
    private var refreshTasks: [String: Task<Void, Never>] = [:]
    /// The newest refresh of each page; an older one finishing does not
    /// clear the page's spinner.
    private var refreshTokens: [String: Int] = [:]
    private var nextToken = 0
    private var searchTask: Task<Void, Never>?
    private var started = false

    /// A page whose report is younger than this is not refreshed when it
    /// comes on screen.
    static let freshFor: TimeInterval = 10 * 60

    /// - Parameters:
    ///   - client: defaults to a client caching under Application Support.
    ///   - location: defaults to CoreLocation.
    ///   - windUnit: defaults to mph where the locale uses it (US, UK).
    ///   - isLive: false to show only cached data (screenshots).
    public init(
        client: WeatherClient? = nil,
        location: (any LocationProviding)? = nil,
        windUnit: WindUnit? = nil,
        isLive: Bool? = nil
    ) {
        self.client = client ?? WeatherClient(storageDirectory: Self.defaultStorageDirectory())
        self.location = location ?? DeviceLocationProvider()
        self.windUnit = windUnit ?? Self.localeWindUnit()
        self.isLive = isLive ?? !UserDefaults.standard.bool(forKey: "WeatherOfflineDemo")
        pages = [WeatherPage(id: WeatherPage.myLocationID)]
    }

    // MARK: Selected page (what most of the UI and the tests look at)

    public var selectedPage: WeatherPage? { pages.first { $0.id == selection } }
    public var place: Place? { selectedPage?.place }
    public var report: WeatherReport? { selectedPage?.report }
    public var isRefreshing: Bool { selectedPage?.isRefreshing ?? false }
    public var errorMessage: String? { selectedPage?.errorMessage }

    // MARK: Lifecycle

    /// Fills every page from the cache at once, then finds the device and
    /// refreshes "My Location". Call from `.task`; later calls do nothing.
    public func start() async {
        guard !started else { return }
        started = true
        var myLocation = WeatherPage(id: WeatherPage.myLocationID)
        if let cached = client.cachedReports.first(where: { $0.place.isDeviceLocation }) {
            myLocation.place = cached.place
            myLocation.report = cached
        }
        pages = [myLocation] + client.savedPlaces.map { place in
            var page = WeatherPage.saved(place)
            page.report = client.cachedReport(for: place)
            return page
        }
        recomputeOverviews()
        if let requested = UserDefaults.standard.string(forKey: "WeatherPage"),
            let index = Int(requested), pages.indices.contains(index)
        {
            selection = pages[index].id
        }
        guard isLive else { return }
        await useCurrentLocation().value
    }

    /// Call when the app comes back to the foreground: the hourly strip moves
    /// on, and a stale page on screen refreshes.
    public func becameActive() {
        recomputeOverviews()
        pageDidAppear()
    }

    /// Call when the page on screen changes: refreshes it if its report is
    /// missing, cached or older than ten minutes.
    public func pageDidAppear() {
        guard isLive, started, let page = selectedPage, !page.isRefreshing else { return }
        if page.isMyLocation && page.place == nil {
            useCurrentLocation()
            return
        }
        let needsRefresh = page.report.map { $0.isCached || $0.age() > Self.freshFor } ?? true
        if needsRefresh, page.place != nil {
            refresh(page.id)
        }
    }

    // MARK: Location

    /// Gets a location fix, moves "My Location" to it and refreshes it.
    @discardableResult
    public func useCurrentLocation() -> Task<Void, Never> {
        locationMessage = nil
        update(WeatherPage.myLocationID) { $0.isRefreshing = true }
        let location = self.location
        return Task {
            do {
                let fix = try await location.currentLocation()
                let place = Place.currentLocation(fix)
                update(WeatherPage.myLocationID) { page in
                    page.place = place
                    // A cached report from far away would be misleading.
                    if let old = page.report, !Self.samePlace(old.place, place, withinDegrees: 0.05) {
                        page.report = client.cachedReport(for: place)
                        page.overview = nil
                    }
                }
                recomputeOverviews()
                await refresh(WeatherPage.myLocationID).value
            } catch {
                let message = (error as? LocationError)?.userMessage
                    ?? LocationError.unavailable(detail: "\(error)").userMessage
                locationMessage = message
                update(WeatherPage.myLocationID) { page in
                    if refreshTasks[page.id] == nil { page.isRefreshing = false }
                }
            }
        }
    }

    // MARK: Refresh

    /// Refreshes one page. Rust runs one refresh at a time, so starting this
    /// cancels another page's refresh; that page keeps what it had.
    @discardableResult
    public func refresh(_ pageID: String? = nil) -> Task<Void, Never> {
        let id = pageID ?? selection
        guard let place = pages.first(where: { $0.id == id })?.place else {
            return Task {}
        }
        refreshTasks[id]?.cancel()
        nextToken += 1
        let token = nextToken
        refreshTokens[id] = token
        update(id) { $0.isRefreshing = true }
        let client = self.client
        let task = Task {
            do {
                let report = try await client.report(for: place)
                update(id) { page in
                    page.report = report
                    page.errorMessage = nil
                }
                recomputeOverview(id)
            } catch is CancellationError {
                // Superseded or cancelled: the page keeps what it had.
            } catch let error as WeatherClientError {
                update(id) { $0.errorMessage = error.userMessage }
            } catch {
                update(id) {
                    $0.errorMessage = WeatherClientError.internalFailure(detail: "\(error)").userMessage
                }
            }
            if refreshTokens[id] == token {
                update(id) { $0.isRefreshing = false }
                refreshTasks[id] = nil
                refreshTokens[id] = nil
            }
        }
        refreshTasks[id] = task
        return task
    }

    /// Cancels the refresh of the page on screen. The page keeps what it had.
    public func cancelRefresh() {
        let id = selection
        refreshTasks[id]?.cancel()
        refreshTasks[id] = nil
        refreshTokens[id] = nil
        update(id) { $0.isRefreshing = false }
    }

    // MARK: Saved places

    /// Saves `place` (or finds it already saved), shows its page and
    /// refreshes it.
    public func add(_ place: Place) {
        do {
            let saved = try client.save(place)
            rebuildPages(saved: saved)
            if let match = saved.first(where: { Self.samePlace($0, place, withinDegrees: 0.02) }) {
                selection = WeatherPage.pageID(for: match)
            }
            searchResults = []
            searchMessage = nil
            refresh(selection)
        } catch {
            searchMessage = (error as? WeatherClientError)?.userMessage ?? "\(error)"
        }
    }

    /// Removes saved pages. "My Location" cannot be removed.
    public func remove(_ pageIDs: [String]) {
        for page in pages where pageIDs.contains(page.id) && !page.isMyLocation {
            guard let place = page.place else { continue }
            refreshTasks[page.id]?.cancel()
            refreshTasks[page.id] = nil
            refreshTokens[page.id] = nil
            if let saved = try? client.removeSaved(place) {
                rebuildPages(saved: saved)
            }
        }
        if selectedPage == nil { selection = pages.first?.id ?? WeatherPage.myLocationID }
    }

    /// Moves a saved page; indexes count saved places only (not "My Location").
    public func moveSaved(from: Int, to: Int) {
        if let saved = try? client.moveSaved(from: from, to: to) {
            rebuildPages(saved: saved)
        }
    }

    // MARK: Search

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

    /// Stops everything. The cache and saved places are already on disk.
    public func close() {
        refreshTasks.values.forEach { $0.cancel() }
        searchTask?.cancel()
        client.shutdown()
    }

    // MARK: Helpers

    private func update(_ id: String, _ change: (inout WeatherPage) -> Void) {
        guard let index = pages.firstIndex(where: { $0.id == id }) else { return }
        change(&pages[index])
    }

    private func recomputeOverview(_ id: String) {
        let now = Date()
        update(id) { page in
            page.overview = page.report.map { client.overview(of: $0, at: now, windUnit: windUnit) }
        }
    }

    private func recomputeOverviews() {
        for page in pages { recomputeOverview(page.id) }
    }

    /// Keeps "My Location" and every page's state; adds and drops pages to
    /// match `saved`.
    private func rebuildPages(saved: [Place]) {
        let existing = Dictionary(pages.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let myLocation = existing[WeatherPage.myLocationID] ?? WeatherPage(id: WeatherPage.myLocationID)
        pages = [myLocation] + saved.map { place in
            let id = WeatherPage.pageID(for: place)
            if let page = existing[id] { return page }
            var page = WeatherPage.saved(place)
            page.report = client.cachedReport(for: place)
            page.overview = page.report.map { client.overview(of: $0, windUnit: windUnit) }
            return page
        }
    }

    static func samePlace(_ a: Place, _ b: Place, withinDegrees limit: Double = 0.01) -> Bool {
        abs(a.coordinate.latitude - b.coordinate.latitude) < limit
            && abs(a.coordinate.longitude - b.coordinate.longitude) < limit
    }

    private static func localeWindUnit() -> WindUnit {
        switch Locale.current.measurementSystem {
        case .us, .uk: return .milesPerHour
        default: return .kilometresPerHour
        }
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
