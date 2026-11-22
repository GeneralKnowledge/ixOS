import ForgeKit
import Foundation
import Observation

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
        // Shown at launch until the refresh lands; only worth saying when old.
        case .none: return ageSeconds < 3_600 ? nil : "Updated \(age) ago."
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
@Observable
@MainActor
public final class WeatherViewModel {
    /// "My Location" first, then the saved places in the user's order.
    public private(set) var pages: [WeatherPage]
    /// The page on screen.
    public var selection: String = WeatherPage.myLocationID
    /// Why "My Location" did not work.
    public private(set) var locationMessage: String?
    /// The person has turned location off for Weather. Asking again does
    /// nothing after a denial, so the screen offers Settings instead.
    public private(set) var locationDenied = false

    public private(set) var searchResults: [Place] = []
    public private(set) var isSearching = false
    public private(set) var searchMessage: String?
    /// Why removing or reordering a saved place failed; the places list
    /// shows it, and clears it when dismissed.
    public var placesError: String?
    /// Why the saved places couldn't be read (they are left untouched).
    public private(set) var placesProblem: SavedDataProblem?

    public let client: WeatherClient
    /// The person's display units; every value on the page uses them.
    public private(set) var units: UnitSettings
    /// Why saving the units failed, shown in the units screen.
    public var unitsError: String?
    private let location: any LocationProviding
    /// False shows only what is cached: no location request, no network.
    /// The simulator screenshot job uses it (launch argument
    /// `-WeatherOfflineDemo YES`).
    private let isLive: Bool
    @ObservationIgnored private var refreshTasks: [String: Task<Void, Never>] = [:]
    /// The newest refresh of each page; an older one finishing does not
    /// clear the page's spinner.
    @ObservationIgnored private var refreshTokens: [String: Int] = [:]
    @ObservationIgnored private var nextToken = 0
    @ObservationIgnored private var searchTask: Task<Void, Never>?
    /// Bumped by every search and clear: a search whose number is no longer
    /// current was replaced or cleared, and must not write its results.
    @ObservationIgnored private var searchGeneration = 0
    @ObservationIgnored private var started = false
    /// When "My Location" last got a fix. The process can stay alive for
    /// days while the phone travels, so an old fix is looked up again.
    @ObservationIgnored private var lastFix: Date?
    /// The clock, so tests can move time on.
    @ObservationIgnored var now: () -> Date = Date.init

    /// A page whose report is younger than this is not refreshed when it
    /// comes on screen; a location fix older than this is looked up again.
    static let freshFor: TimeInterval = 10 * 60

    /// - Parameters:
    ///   - client: the Rust core the app's host opened, wrapped.
    ///   - location: defaults to the core's `current_location` (Core
    ///     Location through Forge's `location` service).
    ///   - units: defaults to the saved units, else the region's usual ones.
    ///   - isLive: false to show only cached data (screenshots).
    public init(
        client: WeatherClient,
        location: (any LocationProviding)? = nil,
        units: UnitSettings? = nil,
        isLive: Bool? = nil
    ) {
        self.client = client
        self.location = location ?? DeviceLocationProvider(core: client.app)
        self.units = units ?? client.units(regionCode: Locale.current.region?.identifier)
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
        placesProblem = client.savedPlacesProblem
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

    /// Follows the app's phases through `host`, for as long as this model
    /// lives: each return from the background calls `becameActive`.
    public func follow<Core: ForgeCore>(_ host: ForgeHost<Core>) {
        host.onResume(self) { [weak self] _ in self?.becameActive() }
    }

    /// The app came back from the background (`follow`): the hourly strip
    /// moves on, and a stale page on screen refreshes, finding the phone
    /// again first if its fix is old.
    public func becameActive() {
        recomputeOverviews()
        pageDidAppear()
    }

    /// Call when the page on screen changes: refreshes it if its report is
    /// missing, cached or older than ten minutes. "My Location" first finds
    /// the phone again if its fix is that old: the phone may have moved
    /// while the app was in the background.
    public func pageDidAppear() {
        guard isLive, started, let page = selectedPage, !page.isRefreshing else { return }
        if page.isMyLocation && (page.place == nil || needsNewFix) {
            useCurrentLocation()
            return
        }
        refreshIfStale(page.id)
    }

    /// Refreshes a page whose report is missing, cached or older than
    /// `freshFor`, if it has a place.
    private func refreshIfStale(_ id: String) {
        guard let page = pages.first(where: { $0.id == id }), page.place != nil,
            refreshTasks[id] == nil
        else { return }
        if page.report.map({ $0.isCached || $0.age() > Self.freshFor }) ?? true {
            refresh(id)
        }
    }

    /// What to say above a page's report: why its refresh failed, why "My
    /// Location" couldn't be found again (the report is then the last
    /// place found), or how old a cached report is. Nil for nothing.
    public func notice(for page: WeatherPage) -> String? {
        if let error = page.errorMessage { return error }
        if page.isMyLocation, page.report != nil, let location = locationMessage {
            return "\(location) Showing the last place found."
        }
        return page.staleNotice
    }

    // MARK: Location

    /// Gets a location fix, moves "My Location" to it and refreshes it.
    @discardableResult
    public func useCurrentLocation() -> Task<Void, Never> {
        locationMessage = nil
        locationDenied = false
        update(WeatherPage.myLocationID) { $0.isRefreshing = true }
        let location = self.location
        return Task {
            do {
                let fix = try await location.currentLocation()
                lastFix = now()
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
            } catch is CancellationError {
                // The app left the screen: it asks again on return.
                update(WeatherPage.myLocationID) { page in
                    if refreshTasks[page.id] == nil { page.isRefreshing = false }
                }
            } catch {
                let message = (error as? LocationError)?.userMessage
                    ?? LocationError.unavailable(detail: "\(error)").userMessage
                locationMessage = message
                locationDenied = Self.isDenial(error)
                update(WeatherPage.myLocationID) { page in
                    if refreshTasks[page.id] == nil { page.isRefreshing = false }
                }
                // The last place found stays on screen, with the reason
                // above it (`notice(for:)`), and its forecast kept current.
                refreshIfStale(WeatherPage.myLocationID)
            }
        }
    }

    private static func isDenial(_ error: Error) -> Bool {
        switch error {
        case LocationError.denied, WeatherClientError.location(.denied): return true
        default: return false
        }
    }

    /// Whether "My Location" should be looked up again before refreshing:
    /// never found this run, or found more than `freshFor` ago.
    private var needsNewFix: Bool {
        guard let lastFix else { return true }
        return now().timeIntervalSince(lastFix) > Self.freshFor
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
    /// refreshes it. False when it couldn't be saved: `searchMessage` says
    /// why, so the search sheet should stay open to show it.
    @discardableResult
    public func add(_ place: Place) -> Bool {
        do {
            let saved = try client.save(place)
            rebuildPages(saved: saved)
            if let match = saved.first(where: { Self.samePlace($0, place, withinDegrees: 0.02) }) {
                selection = WeatherPage.pageID(for: match)
            }
            searchResults = []
            searchMessage = nil
            refresh(selection)
            return true
        } catch {
            searchMessage = Self.message(error, doing: "add \(place.name)")
            return false
        }
    }

    /// Removes saved pages. "My Location" cannot be removed.
    public func remove(_ pageIDs: [String]) {
        for page in pages where pageIDs.contains(page.id) && !page.isMyLocation {
            guard let place = page.place else { continue }
            refreshTasks[page.id]?.cancel()
            refreshTasks[page.id] = nil
            refreshTokens[page.id] = nil
            do {
                rebuildPages(saved: try client.removeSaved(place))
            } catch {
                placesError = Self.message(error, doing: "remove \(place.name)")
            }
        }
        if selectedPage == nil { selection = pages.first?.id ?? WeatherPage.myLocationID }
    }

    /// Moves a saved page; indexes count saved places only (not "My Location").
    public func moveSaved(from: Int, to: Int) {
        do {
            rebuildPages(saved: try client.moveSaved(from: from, to: to))
        } catch {
            placesError = Self.message(error, doing: "move the place")
        }
    }

    /// After the person agreed: sets the unreadable list aside (kept) and
    /// starts a new, empty one.
    public func startNewSavedPlaces() {
        do {
            rebuildPages(saved: try client.startNewSavedPlaces())
            placesProblem = client.savedPlacesProblem
        } catch {
            placesError = Self.message(error, doing: "start a new list")
        }
    }

    /// "Couldn't remove Paris." and why, for the places list's alert.
    nonisolated static func message(_ error: Error, doing action: String) -> String {
        let reason = (error as? WeatherClientError)?.userMessage
            ?? WeatherClientError.internalFailure(detail: "\(error)").userMessage
        return "Couldn't \(action). \(reason)"
    }

    // MARK: Search

    /// Clears results that no longer match the query (it got too short to
    /// search), and stops a search still running.
    public func clearSearch() {
        searchGeneration += 1
        searchTask?.cancel()
        searchResults = []
        searchMessage = nil
        isSearching = false
    }

    /// Searches for places by name. A newer search replaces an older one.
    @discardableResult
    public func search(_ query: String) -> Task<Void, Never> {
        searchGeneration += 1
        let generation = searchGeneration
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
            let outcome: (places: [Place], message: String?)
            do {
                let places = try await client.searchPlaces(trimmed)
                outcome = (places, places.isEmpty ? "No places match “\(trimmed)”." : nil)
            } catch is CancellationError {
                return
            } catch let error as WeatherClientError {
                outcome = ([], error.userMessage)
            } catch {
                outcome = ([], "\(error)")
            }
            // Replaced by a newer search or cleared while this one ran: its
            // answer is for a query that is no longer on screen.
            guard generation == searchGeneration else { return }
            searchResults = outcome.places
            searchMessage = outcome.message
            isSearching = false
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
            page.overview = page.report.map { client.overview(of: $0, at: now, units: units) }
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
            page.overview = page.report.map { client.overview(of: $0, units: units) }
            return page
        }
    }

    static func samePlace(_ a: Place, _ b: Place, withinDegrees limit: Double = 0.01) -> Bool {
        abs(a.coordinate.latitude - b.coordinate.latitude) < limit
            && abs(a.coordinate.longitude - b.coordinate.longitude) < limit
    }

    /// Saves `units` and shows every page in them. On failure nothing
    /// changes and `unitsError` says why.
    public func setUnits(_ new: UnitSettings) {
        do {
            try client.setUnits(new)
            units = new
            unitsError = nil
            recomputeOverviews()
        } catch {
            unitsError = "Couldn't save your units."
        }
    }

    /// A stored value in the person's units ("12°", "18 mph").
    public func format(_ quantity: Quantity) -> String {
        client.format(quantity, units: units)
    }
}
