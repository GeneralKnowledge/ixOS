import SwiftUI
@preconcurrency import WeatherFFI

private struct WeatherUnitsKey: EnvironmentKey {
    static let defaultValue: UnitSettings = defaultUnits(regionCode: nil)
}

extension EnvironmentValues {
    /// The person's display units, set once at the top of the app so every
    /// view that shows a value uses the same ones.
    var weatherUnits: UnitSettings {
        get { self[WeatherUnitsKey.self] }
        set { self[WeatherUnitsKey.self] = newValue }
    }
}
