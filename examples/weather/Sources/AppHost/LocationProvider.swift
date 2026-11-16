import Foundation
@preconcurrency import WeatherFFI

/// One location reading for "My Location".
public struct LocationReading: Sendable, Equatable {
    public var coordinate: Coordinate
    /// Radius of uncertainty in metres, when the platform reports one.
    public var horizontalAccuracy: Double?
    /// The town or city the platform's reverse geocoder found, if any.
    public var placeName: String?

    public init(coordinate: Coordinate, horizontalAccuracy: Double? = nil, placeName: String? = nil) {
        self.coordinate = coordinate
        self.horizontalAccuracy = horizontalAccuracy
        self.placeName = placeName
    }
}

public enum LocationError: Error, Sendable, Equatable {
    /// The user said no (or turned location off for this app), or parental
    /// controls forbid it.
    case denied
    /// No fix could be obtained (indoors, no signal, ...).
    case unavailable(detail: String)

    public var userMessage: String {
        switch self {
        case .denied:
            return "Location access is off for Weather. Search for a city, or allow location in Settings."
        case .unavailable:
            return "Your location isn't available right now. Search for a city instead."
        }
    }
}

/// Where the app's "current location" comes from: the Rust core in the app,
/// a fake in tests.
public protocol LocationProviding: Sendable {
    func currentLocation() async throws -> LocationReading
}

/// The device's position, from Rust's `current_location`, which asks Core
/// Location through Forge's `location` service (permission prompt
/// included). The place's name comes from the platform's reverse geocoder
/// (`PlaceNames`, iOS only).
public struct DeviceLocationProvider: LocationProviding {
    private let core: UncheckedSendable<ForgeWeatherApp>
    private let name: @Sendable (Coordinate) async -> String?

    public init(core: ForgeWeatherApp, name: @escaping @Sendable (Coordinate) async -> String? = PlaceNames.locality) {
        self.core = UncheckedSendable(core)
        self.name = name
    }

    public func currentLocation() async throws -> LocationReading {
        let fix: DeviceFix
        do {
            fix = try await core.value.currentLocation()
        } catch WeatherError.LocationDenied {
            throw LocationError.denied
        } catch let WeatherError.LocationUnavailable(reason) {
            throw LocationError.unavailable(detail: reason)
        } catch WeatherError.Cancelled {
            throw CancellationError()
        } catch {
            throw LocationError.unavailable(detail: String(describing: error))
        }
        return LocationReading(
            coordinate: fix.coordinate,
            horizontalAccuracy: fix.horizontalAccuracyM,
            placeName: await name(fix.coordinate))
    }
}
