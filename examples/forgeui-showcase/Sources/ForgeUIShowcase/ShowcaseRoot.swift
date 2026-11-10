#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

/// The showcase app: a gallery of every ForgeUI component, and three
/// example screens built only from ForgeUI and SwiftUI. The appearance
/// menu in each tab switches the theme, light/dark and the text size, so
/// every component can be seen in every combination.
public struct ShowcaseRoot: View {
    @State private var theme = ShowcaseTheme.standard
    @State private var appearance = ShowcaseAppearance.system
    @State private var textSize = ShowcaseTextSize.system
    @State private var tasks = TasksModel()

    public init() {}

    public var body: some View {
        TabView {
            GalleryView(theme: $theme, appearance: $appearance, textSize: $textSize)
                .tabItem { Label("Components", systemImage: "square.grid.2x2") }
                .accessibilityIdentifier("tab.components")
            ScreensView(tasks: tasks, theme: $theme, appearance: $appearance, textSize: $textSize)
                .tabItem { Label("Screens", systemImage: "iphone") }
                .accessibilityIdentifier("tab.screens")
        }
        .forgeTheme(theme.theme)
        .preferredColorScheme(appearance.colorScheme)
        .modifier(TextSizeOverride(size: textSize))
    }
}

extension ShowcaseAppearance {
    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

private struct TextSizeOverride: ViewModifier {
    let size: ShowcaseTextSize

    @ViewBuilder
    func body(content: Content) -> some View {
        switch size {
        case .system: content
        case .small: content.dynamicTypeSize(.small)
        case .large: content.dynamicTypeSize(.xxxLarge)
        case .accessibility: content.dynamicTypeSize(.accessibility3)
        }
    }
}

/// The toolbar menu that switches theme, appearance and text size.
struct AppearanceMenu: View {
    @Binding var theme: ShowcaseTheme
    @Binding var appearance: ShowcaseAppearance
    @Binding var textSize: ShowcaseTextSize

    var body: some View {
        Menu {
            Picker("Theme", selection: $theme) {
                ForEach(ShowcaseTheme.allCases) { Text($0.title).tag($0) }
            }
            Picker("Appearance", selection: $appearance) {
                ForEach(ShowcaseAppearance.allCases) { Text($0.title).tag($0) }
            }
            Picker("Text Size", selection: $textSize) {
                ForEach(ShowcaseTextSize.allCases) { Text($0.title).tag($0) }
            }
        } label: {
            Label("Appearance", systemImage: "paintpalette")
        }
        .accessibilityIdentifier("showcase.appearance")
    }
}
#endif
