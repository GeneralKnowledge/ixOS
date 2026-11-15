#if canImport(SwiftUI)
import SwiftUI

/// Dark or light mode for one launch, without changing the phone's setting:
/// `-ForgeAppearance dark` or `-ForgeAppearance light` as a launch argument
/// (`forge ui launch --appearance`, a UI test's `launch = { args = [...]
/// }`). Without it the app follows the phone. A launch argument also lands
/// in `UserDefaults`, which is where it is read. Attach once, to the root
/// view.
extension View {
    public func forgeLaunchAppearance() -> some View {
        preferredColorScheme(ForgeLaunchAppearance.scheme)
    }
}

/// The launch argument `forgeLaunchAppearance()` reads.
public enum ForgeLaunchAppearance {
    public static let argument = "ForgeAppearance"

    /// The scheme the launch argument asks for, or nil to follow the phone.
    public static var scheme: ColorScheme? {
        switch UserDefaults.standard.string(forKey: argument) {
        case "dark": return .dark
        case "light": return .light
        default: return nil
        }
    }
}
#endif
