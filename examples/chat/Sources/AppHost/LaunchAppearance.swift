import SwiftUI

/// Dark or light mode for one launch, without changing the phone's setting:
/// `-ForgeAppearance dark` or `-ForgeAppearance light` as a launch argument
/// (`forge_launch_app appearance`, `forge ui explore --appearance`, a UI
/// test's `launch = { args = [...] }`). Without it the app follows the
/// phone. A launch argument also lands in `UserDefaults`, which is where it
/// is read. Attach once, to the root view.
extension View {
    public func forgeLaunchAppearance() -> some View {
        preferredColorScheme(ForgeLaunchAppearance.scheme)
    }
}

enum ForgeLaunchAppearance {
    static let argument = "ForgeAppearance"

    static var scheme: ColorScheme? {
        switch UserDefaults.standard.string(forKey: argument) {
        case "dark": return .dark
        case "light": return .light
        default: return nil
        }
    }
}
