// Forge native services: location and heading (optional: `native_services!(location)`).
// Source of truth: crates/forge-services/swift/optional/. Copied into an app's
// Generated/ForgeFFI only when its Rust crate selects the service.
//
// "When in use" only. Needs NSLocationWhenInUseUsageDescription (forge.toml
// `[ios.permissions] location_when_in_use`) and NativePermission.location.

import Foundation

#if os(iOS)
    import CoreLocation
#endif

extension ForgeServices {
    static func installLocation() throws {
        installNativeLocation(source: ForgeLocationSource.shared)
        #if os(iOS)
            ForgePermissions.shared.register(
                LocationPermissionAdapter(source: ForgeLocationSource.shared), for: .location)
        #endif
    }
}

#if os(iOS)

    /// `LocationSource` backed by one `CLLocationManager`, created on the main
    /// thread (from `ForgeServices.install()`), so Core Location calls its
    /// delegate there.
    public final class ForgeLocationSource: NSObject, LocationSource, CLLocationManagerDelegate,
        @unchecked Sendable
    {
        public static let shared = ForgeLocationSource()

        private let manager: CLLocationManager
        private let lock = NSLock()
        private var sink: LocationSink?
        private var authorizationWaiters: [CheckedContinuation<Void, Never>] = []

        private override init() {
            manager = CLLocationManager()
            super.init()
            manager.delegate = self
        }

        // MARK: LocationSource

        public func lastKnown() -> LocationFix? {
            manager.location.map(Self.fix)
        }

        public func isPrecise() -> Bool {
            manager.accuracyAuthorization == .fullAccuracy
        }

        public func headingAvailable() -> Bool {
            CLLocationManager.headingAvailable()
        }

        public func start(options: LocationOptions, sink: LocationSink) throws {
            guard permissionStatus() == .granted else {
                throw NativeServiceError.PermissionDenied(
                    reason: "location is not allowed for this app: request NativePermission.location first")
            }
            lock.lock()
            self.sink = sink
            lock.unlock()
            onMain { [manager] in
                manager.desiredAccuracy = Self.accuracy(options.accuracy)
                manager.distanceFilter = options.distanceFilterM ?? kCLDistanceFilterNone
                manager.startUpdatingLocation()
                if options.includeHeading && CLLocationManager.headingAvailable() {
                    manager.headingFilter = 1
                    manager.startUpdatingHeading()
                }
            }
        }

        public func stop() {
            lock.lock()
            sink = nil
            lock.unlock()
            onMain { [manager] in
                manager.stopUpdatingLocation()
                manager.stopUpdatingHeading()
            }
        }

        // MARK: Permission

        func permissionStatus() -> PermissionStatus {
            switch manager.authorizationStatus {
            case .notDetermined: return .notDetermined
            case .restricted: return .restricted
            case .denied: return .denied
            case .authorizedAlways, .authorizedWhenInUse: return .granted
            @unknown default: return .unsupported
            }
        }

        func requestWhenInUse() async -> PermissionStatus {
            guard permissionStatus() == .notDetermined else { return permissionStatus() }
            guard
                Bundle.main.object(forInfoDictionaryKey: "NSLocationWhenInUseUsageDescription") != nil
            else {
                // Without it iOS ignores the request and no answer would come.
                print("[forge] location permission: add [ios.permissions] location_when_in_use to forge.toml first")
                return .unsupported
            }
            // The delegate hears the answer; a spurious early callback still
            // reads notDetermined, so wait once more.
            for _ in 0..<2 where permissionStatus() == .notDetermined {
                await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
                    lock.lock()
                    authorizationWaiters.append(done)
                    lock.unlock()
                    onMain { [manager] in manager.requestWhenInUseAuthorization() }
                }
            }
            return permissionStatus()
        }

        // MARK: CLLocationManagerDelegate

        public func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
            guard let sink = currentSink() else { return }
            for location in locations {
                sink.send(event: .fix(fix: Self.fix(location)))
            }
        }

        public func locationManager(_ manager: CLLocationManager, didUpdateHeading newHeading: CLHeading) {
            guard let sink = currentSink() else { return }
            sink.send(
                event: .heading(
                    heading: Heading(
                        magneticDegrees: newHeading.magneticHeading,
                        trueDegrees: newHeading.trueHeading >= 0 ? newHeading.trueHeading : nil,
                        accuracyDegrees: newHeading.headingAccuracy >= 0 ? newHeading.headingAccuracy : nil,
                        timestampMs: Self.unixMs(newHeading.timestamp))))
        }

        public func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
            let code = (error as NSError).code
            // "Location unknown" is transient: Core Location keeps trying.
            if code == CLError.Code.locationUnknown.rawValue { return }
            currentSink()?.send(
                event: .failed(
                    reason: error.localizedDescription,
                    permissionDenied: code == CLError.Code.denied.rawValue))
        }

        public func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
            lock.lock()
            let waiters = authorizationWaiters
            authorizationWaiters = []
            lock.unlock()
            for waiter in waiters { waiter.resume() }
        }

        // MARK: Helpers

        private func currentSink() -> LocationSink? {
            lock.lock()
            defer { lock.unlock() }
            return sink
        }

        private func onMain(_ work: @escaping () -> Void) {
            if Thread.isMainThread { work() } else { DispatchQueue.main.async(execute: work) }
        }

        static func accuracy(_ accuracy: LocationAccuracy) -> CLLocationAccuracy {
            switch accuracy {
            case .best: return kCLLocationAccuracyBest
            case .nearestTenMeters: return kCLLocationAccuracyNearestTenMeters
            case .hundredMeters: return kCLLocationAccuracyHundredMeters
            case .kilometer: return kCLLocationAccuracyKilometer
            case .reduced: return kCLLocationAccuracyReduced
            }
        }

        static func unixMs(_ date: Date) -> Int64 {
            Int64(date.timeIntervalSince1970 * 1000)
        }

        /// Core Location marks unknown values with negative accuracies.
        static func fix(_ l: CLLocation) -> LocationFix {
            LocationFix(
                latitude: l.coordinate.latitude,
                longitude: l.coordinate.longitude,
                horizontalAccuracyM: l.horizontalAccuracy,
                altitudeM: l.verticalAccuracy > 0 ? l.altitude : nil,
                verticalAccuracyM: l.verticalAccuracy > 0 ? l.verticalAccuracy : nil,
                speedMps: l.speed >= 0 ? l.speed : nil,
                speedAccuracyMps: l.speedAccuracy >= 0 ? l.speedAccuracy : nil,
                courseDegrees: l.course >= 0 ? l.course : nil,
                courseAccuracyDegrees: l.courseAccuracy >= 0 ? l.courseAccuracy : nil,
                floor: l.floor.map { Int32($0.level) },
                timestampMs: unixMs(l.timestamp))
        }
    }

    struct LocationPermissionAdapter: ForgePermissionAdapter {
        let source: ForgeLocationSource

        func status() async -> PermissionStatus { source.permissionStatus() }

        func request() async -> PermissionStatus { await source.requestWhenInUse() }
    }

#else

    /// Location is wired up on iOS only; elsewhere it reports unavailable.
    public final class ForgeLocationSource: LocationSource, @unchecked Sendable {
        public static let shared = ForgeLocationSource()

        private init() {}

        public func lastKnown() -> LocationFix? { nil }
        public func isPrecise() -> Bool { false }
        public func headingAvailable() -> Bool { false }

        public func start(options: LocationOptions, sink: LocationSink) throws {
            throw NativeServiceError.Unavailable(reason: "location is implemented for iOS only")
        }

        public func stop() {}
    }

#endif
