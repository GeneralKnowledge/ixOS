// The app's host: ForgeKit opens the Rust core once per process, reports
// the application lifecycle to it (refreshes and searches stop when the app
// leaves the screen) and shuts it down. Entry points import WeatherApp only;
// ForgeKit comes with it.
@_exported import ForgeKit
@preconcurrency import WeatherFFI
import Foundation

extension ForgeWeatherApp: ForgeCore {}

extension WeatherError: ForgeKindedError {
    /// The framework kind of each error (`weather_error_kind` in Rust).
    public var forgeKind: ErrorKind { weatherErrorKind(error: self) }
}

/// The host for Weather's core. Make it once, in the `App`, never in a view.
/// Saved places and the offline cache stay in Application Support/Weather,
/// where Weather kept them before it ran on ForgeKit; launched with
/// `-ForgeTemporaryData` (UI tests, Device Lab's test runs) it starts on
/// throwaway data instead.
@MainActor
public func makeAppHost() -> ForgeHost<ForgeWeatherApp> {
    ForgeHost(environment: weatherEnvironment) { environment in
        try ForgeWeatherApp(storageDirectory: environment.dataDirectory)
    }
}

/// A host on a fresh temporary folder, for tests.
@MainActor
public func makeTemporaryAppHost() -> ForgeHost<ForgeWeatherApp> {
    ForgeHost(environment: { try ForgeEnvironment.temporary() }) { environment in
        try ForgeWeatherApp(storageDirectory: environment.dataDirectory)
    }
}

@Sendable
func weatherEnvironment() throws -> ForgeEnvironment {
    let standard = try ForgeEnvironment.standard(app: "Weather")
    if ForgeEnvironment.usesTemporaryData() { return standard }
    let fm = FileManager.default
    let support = try fm.url(
        for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
    let data = support.appendingPathComponent("Weather", isDirectory: true)
    try fm.createDirectory(at: data, withIntermediateDirectories: true)
    return ForgeEnvironment(dataDirectory: data.path, cachesDirectory: standard.cachesDirectory)
}
