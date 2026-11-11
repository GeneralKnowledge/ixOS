@preconcurrency import CoreLocation
import Foundation
@preconcurrency import WeatherFFI

/// One location reading from the platform.
public struct LocationFix: Sendable, Equatable {
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
    /// The user said no (or turned location off for this app).
    case denied
    /// Parental controls or a device profile forbid location access.
    case restricted
    /// No fix could be obtained (indoors, simulator without a location, ...).
    case unavailable(detail: String)

    public var userMessage: String {
        switch self {
        case .denied:
            return "Location access is off for Weather. Search for a city, or allow location in Settings."
        case .restricted:
            return "Location access is restricted on this device. Search for a city instead."
        case .unavailable:
            return "Your location isn't available right now. Search for a city instead."
        }
    }
}

/// Where the app's "current location" comes from. Location stays in Swift:
/// Rust only ever receives a coordinate.
public protocol LocationProviding: Sendable {
    func currentLocation() async throws -> LocationFix
}

/// One-shot CoreLocation fix with when-in-use permission.
///
/// Asks for permission the first time, then calls `requestLocation()`.
/// Concurrent callers share the same request.
@MainActor
public final class DeviceLocationProvider: NSObject, LocationProviding {
    private let manager = CLLocationManager()
    private var waiting: [CheckedContinuation<LocationFix, any Error>] = []

    public override init() {
        super.init()
        manager.delegate = self
        // A forecast needs a city, not a street: this is fast and battery-cheap.
        manager.desiredAccuracy = kCLLocationAccuracyKilometer
    }

    public func currentLocation() async throws -> LocationFix {
        var fix = try await withCheckedThrowingContinuation { continuation in
            waiting.append(continuation)
            if waiting.count == 1 {
                proceed(with: manager.authorizationStatus)
            }
        }
        fix.placeName = await Self.locality(of: fix.coordinate)
        return fix
    }

    /// The town or city at `coordinate` ("Bathgate"), from the platform's
    /// reverse geocoder. Nil when it is offline, slow or finds nothing: the
    /// page then says "My Location".
    private static func locality(of coordinate: Coordinate) async -> String? {
        let location = CLLocation(latitude: coordinate.latitude, longitude: coordinate.longitude)
        let geocoder = CLGeocoder()
        let timeout = Task {
            try? await Task.sleep(for: .seconds(5))
            geocoder.cancelGeocode()
        }
        defer { timeout.cancel() }
        guard let mark = try? await geocoder.reverseGeocodeLocation(location).first else {
            return nil
        }
        return mark.locality ?? mark.subAdministrativeArea ?? mark.administrativeArea
    }

    private func proceed(with status: CLAuthorizationStatus) {
        guard !waiting.isEmpty else { return }
        switch status {
        case .notDetermined:
            // The answer arrives in `locationManagerDidChangeAuthorization`.
            manager.requestWhenInUseAuthorization()
        case .denied:
            finish(.failure(LocationError.denied))
        case .restricted:
            finish(.failure(LocationError.restricted))
        default:
            manager.requestLocation()
        }
    }

    private func finish(_ result: Result<LocationFix, LocationError>) {
        let continuations = waiting
        waiting.removeAll()
        for continuation in continuations {
            continuation.resume(with: result.mapError { $0 as any Error })
        }
    }
}

// CoreLocation calls the delegate on the thread the manager was created on,
// which is the main thread here (the class is main-actor isolated).
extension DeviceLocationProvider: CLLocationManagerDelegate {
    nonisolated public func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        let status = manager.authorizationStatus
        MainActor.assumeIsolated {
            // Ignore the "not determined" callback that arrives right after
            // the delegate is set; wait for the user's answer.
            if status != .notDetermined {
                proceed(with: status)
            }
        }
    }

    nonisolated public func locationManager(
        _ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]
    ) {
        guard let location = locations.last else { return }
        let coordinate = Coordinate(
            latitude: location.coordinate.latitude,
            longitude: location.coordinate.longitude)
        let accuracy = location.horizontalAccuracy >= 0 ? location.horizontalAccuracy : nil
        MainActor.assumeIsolated {
            finish(.success(LocationFix(coordinate: coordinate, horizontalAccuracy: accuracy)))
        }
    }

    nonisolated public func locationManager(
        _ manager: CLLocationManager, didFailWithError error: any Error
    ) {
        let denied = (error as? CLError)?.code == .denied
        let detail = error.localizedDescription
        MainActor.assumeIsolated {
            finish(.failure(denied ? .denied : .unavailable(detail: detail)))
        }
    }
}
