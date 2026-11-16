@preconcurrency import WeatherFFI
import SwiftUI

/// Forge's native services for the Rust core: URLSession for forecasts and
/// searches (`http`), Core Location for "My Location" (`location`), and
/// permissions. The App calls `install()` once, in its `init`.
public enum AppServices {
    @MainActor
    public static func install() {
        _ = ForgeServices.install()
    }
}
