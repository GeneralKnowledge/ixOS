import Foundation
@preconcurrency import ForgeFFI

/// The Swift face of one Rust weather instance (`ForgeWeatherApp`).
///
/// Concurrency contract (see `rust/src/weather/app.rs`):
/// - Every method is safe from any task or actor, including the main actor.
///   Nothing here blocks: HTTP runs on Rust worker threads.
/// - One refresh at a time: starting a refresh cancels the one in flight,
///   whose caller then gets `CancellationError`.
/// - Cancelling the calling task cancels the refresh at once. The Rust worker
///   may still be waiting on the network; its late response is discarded.
/// - When the network fails and Rust has a recent cached report for (about)
///   the same place, `report(for:)` returns that report with
///   `freshness == .cached(ageSeconds:problem:)` instead of throwing.
public final class WeatherClient: @unchecked Sendable {
    // @unchecked: the generated class is not marked Sendable, but the Rust
    // object behind it is (`Send + Sync`), and the class holds nothing else.
    private let app: ForgeWeatherApp

    /// Uses Open-Meteo with default settings. `storageDirectory` holds the
    /// offline cache and is created if missing.
    public init(storageDirectory: String) {
        app = ForgeWeatherApp(storageDirectory: storageDirectory)
    }

    /// Overrides endpoints or timeouts (tests point these at local hosts).
    public init(storageDirectory: String, configuration: WeatherConfig) throws {
        app = try bridged {
            try ForgeWeatherApp.withConfig(storageDirectory: storageDirectory, config: configuration)
        }
    }

    // MARK: Queries (never wait for the network)

    /// The last fetched report, marked `.cached`, or nil. Use it to fill the
    /// screen at launch.
    public var cachedReport: WeatherReport? { app.cachedReport() }

    public var isRefreshing: Bool { app.activeRefresh() != nil }

    public var isShutDown: Bool { app.isShutDown() }

    // MARK: Commands

    /// Fetches the forecast for `place`.
    ///
    /// Cancelling the calling task, or starting another refresh, throws
    /// `CancellationError` here immediately.
    public func report(for place: Place) async throws -> WeatherReport {
        try Task.checkCancellation()
        let operation = UncheckedSendable(try bridged { try app.refresh(place: place) })
        return try await withTaskCancellationHandler {
            try await bridgedAsync { try await operation.value.result() }
        } onCancel: {
            // UniFFI does not forward Swift task cancellation to Rust.
            operation.value.cancel()
        }
    }

    /// Looks places up by name; an empty array means no match.
    ///
    /// A search cannot be stopped once sent, but a cancelled caller gets
    /// `CancellationError` instead of stale results.
    public func searchPlaces(_ query: String) async throws -> [Place] {
        try Task.checkCancellation()
        let places = try await bridgedAsync { try await app.searchPlaces(query: query) }
        try Task.checkCancellation()
        return places
    }

    // MARK: Lifecycle

    /// Cancels any refresh and refuses new work (`.closed`). Idempotent.
    public func shutdown() {
        app.shutdown()
    }
}

/// Carries a generated (non-`Sendable`) UniFFI object across concurrency
/// domains. Sound because the Rust objects behind the generated classes are
/// `Send + Sync` and the classes hold nothing but the pointer.
struct UncheckedSendable<Value>: @unchecked Sendable {
    let value: Value

    init(_ value: Value) {
        self.value = value
    }
}
