import Foundation

/// How Device Lab shows a hosted app: the workshop menu's choices. They
/// change only the hosted app's presentation, never its data, and last until
/// Device Lab quits. Launch arguments set the starting values, so a UI test
/// (`uitests/catalogue.toml`) can shoot a screen the same way every run:
/// `-ForgeAppearance dark`, `-WorkshopTextSize ax5`, `-WorkshopScreen compact`.
struct WorkshopSettings: Equatable {
    var appearance: Appearance = .system
    var textSize: TextSize = .system
    var screen: Screen = .full
    var handle: HandlePosition = .middle

    /// Light, dark, or whatever the phone is set to.
    enum Appearance: String, CaseIterable, Identifiable {
        case system, light, dark
        var id: String { rawValue }

        var name: String {
            switch self {
            case .system: "Match iPhone"
            case .light: "Light"
            case .dark: "Dark"
            }
        }
    }

    /// Dynamic Type sizes worth checking: the default, the largest ordinary
    /// size, and two accessibility sizes (where layouts usually break).
    enum TextSize: String, CaseIterable, Identifiable {
        case system, large, xxxl, ax3, ax5
        var id: String { rawValue }

        var name: String {
            switch self {
            case .system: "Match iPhone"
            case .large: "Default (Large)"
            case .xxxl: "Largest (XXXL)"
            case .ax3: "Accessibility 3"
            case .ax5: "Accessibility 5 (largest)"
            }
        }
    }

    /// The area the app gets. `compact` is the smallest iPhone screen iOS 17
    /// runs on (iPhone SE, 375 × 667 points), centred on this phone, so a
    /// layout that only fits a big phone shows up without a second phone.
    /// It is an approximation: the safe areas are this phone's.
    enum Screen: String, CaseIterable, Identifiable {
        case full, compact
        var id: String { rawValue }

        var name: String {
            switch self {
            case .full: "This iPhone"
            case .compact: "iPhone SE Size (375 × 667)"
            }
        }

        /// Width and height in points, or nil for the whole screen.
        var size: (width: Double, height: Double)? {
            switch self {
            case .full: nil
            case .compact: (375, 667)
            }
        }
    }

    /// Where the workshop button sits on the trailing edge, so it can be
    /// moved off whatever it covers.
    enum HandlePosition: String, CaseIterable, Identifiable {
        case top, middle, bottom
        var id: String { rawValue }

        var name: String {
            switch self {
            case .top: "Top"
            case .middle: "Middle"
            case .bottom: "Bottom"
            }
        }
    }

    static let appearanceArgument = "ForgeAppearance"
    static let textSizeArgument = "WorkshopTextSize"
    static let screenArgument = "WorkshopScreen"

    /// The starting values from launch arguments, which land in
    /// `UserDefaults`. Unknown values fall back to the defaults.
    static func fromLaunch(_ defaults: UserDefaults = .standard) -> WorkshopSettings {
        var settings = WorkshopSettings()
        if let value = defaults.string(forKey: appearanceArgument),
           let appearance = Appearance(rawValue: value) {
            settings.appearance = appearance
        }
        if let value = defaults.string(forKey: textSizeArgument),
           let size = TextSize(rawValue: value) {
            settings.textSize = size
        }
        if let value = defaults.string(forKey: screenArgument),
           let screen = Screen(rawValue: value) {
            settings.screen = screen
        }
        return settings
    }

    /// Whether anything differs from showing the app as it is.
    var changesPresentation: Bool {
        appearance != .system || textSize != .system || screen != .full
    }
}
