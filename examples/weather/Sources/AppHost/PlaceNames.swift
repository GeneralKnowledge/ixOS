import Foundation
@preconcurrency import WeatherFFI
#if canImport(CoreLocation)
@preconcurrency import CoreLocation
#endif

/// Names a position for the "My Location" page.
public enum PlaceNames {
    /// The town or city at `coordinate` ("Bathgate"), from the platform's
    /// reverse geocoder. Nil when it is offline, slow or finds nothing (and
    /// on Linux): the page then says "My Location".
    @Sendable
    public static func locality(_ coordinate: Coordinate) async -> String? {
        #if canImport(CoreLocation)
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
        #else
            return nil
        #endif
    }
}
