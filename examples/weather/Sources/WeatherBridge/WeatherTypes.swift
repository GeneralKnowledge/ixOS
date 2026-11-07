import Foundation
@preconcurrency import ForgeFFI

// The Weather payload is used as UniFFI generates it: records become Swift
// structs, Rust enums with data become Swift enums with associated values, and
// `Option`/`Vec` become optionals and arrays. That is the point of this
// example, so the bridge re-exports those types instead of wrapping them.
// (The generated records and enums are `Sendable`, `Equatable` and `Hashable`.)

public typealias Coordinate = ForgeFFI.Coordinate
public typealias Place = ForgeFFI.Place
public typealias PlaceSource = ForgeFFI.PlaceSource
public typealias WeatherReport = ForgeFFI.WeatherReport
public typealias CurrentConditions = ForgeFFI.CurrentConditions
public typealias HourlyPoint = ForgeFFI.HourlyPoint
public typealias DailyForecast = ForgeFFI.DailyForecast
public typealias Wind = ForgeFFI.Wind
public typealias WeatherCondition = ForgeFFI.WeatherCondition
public typealias Intensity = ForgeFFI.Intensity
public typealias Freshness = ForgeFFI.Freshness
public typealias FetchProblem = ForgeFFI.FetchProblem
public typealias WeatherConfig = ForgeFFI.WeatherConfig
public typealias WeatherOverview = ForgeFFI.WeatherOverview
public typealias TimelineEntry = ForgeFFI.TimelineEntry
public typealias SkyScene = ForgeFFI.SkyScene
public typealias WindUnit = ForgeFFI.WindUnit

extension Place {
    /// A place for a device location fix. Rust only needs the coordinate;
    /// the name is what the UI shows: the locality the platform found for
    /// the fix, else "My Location".
    public static func currentLocation(_ fix: LocationFix) -> Place {
        Place(
            name: fix.placeName ?? "My Location",
            region: nil,
            country: nil,
            countryCode: nil,
            coordinate: fix.coordinate,
            timezone: nil,
            population: nil,
            source: .device(horizontalAccuracyM: fix.horizontalAccuracy)
        )
    }

    /// "Île-de-France, France", or nil when neither is known.
    public var subtitle: String? {
        let parts = [region, country].compactMap { $0 }.filter { !$0.isEmpty }
        return parts.isEmpty ? nil : parts.joined(separator: ", ")
    }

    public var isDeviceLocation: Bool {
        if case .device = source { return true }
        return false
    }
}

extension WeatherReport {
    /// Seconds since this report was fetched.
    public func age(now: Date = Date()) -> TimeInterval {
        max(0, now.timeIntervalSince1970 - TimeInterval(fetchedAtUnix))
    }

    /// True when a refresh failed and fell back to this cached report.
    public var isOfflineFallback: Bool {
        if case .cached(_, .some) = freshness { return true }
        return false
    }

    /// True when this report came from the on-device cache, not the network.
    public var isCached: Bool {
        if case .cached = freshness { return true }
        return false
    }
}
