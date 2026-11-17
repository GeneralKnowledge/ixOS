import SwiftUI

/// A hosted app, full screen, laid out as it is when launched on its own:
/// its own status bar, safe areas and gestures, with nothing of Device Lab's
/// around it except the workshop button on the trailing edge. A sheet (what
/// the Apps screen used before) moved the app down, took its status bar and
/// turned a downward stroke near the top into "close the app" (experience
/// audit X9, friction audit D1 and D3).
///
/// The workshop button goes back to Device Lab, switches to another hosted
/// app, and changes how the app is
/// shown (appearance, text size, a smaller screen) without touching the
/// app's data or the phone's settings. UI tests mask it by its identifier,
/// `workshop.menu` (`uitests/catalogue.toml`).
struct HostedAppView: View {
    let app: EmbeddedApp
    @Binding var settings: WorkshopSettings
    /// Opens another hosted app in this one's place.
    let switchTo: (EmbeddedApp) -> Void
    let close: () -> Void
    /// The phone's own text size, used when the workshop doesn't override it.
    @Environment(\.dynamicTypeSize) private var phoneTextSize

    var body: some View {
        // One view tree whatever the settings, so changing one keeps the
        // app's own state (its tab, its navigation path, a half-typed field).
        ZStack {
            Color.gray
                .opacity(settings.screen.size == nil ? 0 : 0.25)
                .ignoresSafeArea()
                .accessibilityHidden(true)
            app.content
                .dynamicTypeSize(settings.textSize.dynamicTypeSize ?? phoneTextSize)
                .frame(
                    width: settings.screen.size.map { CGFloat($0.width) },
                    height: settings.screen.size.map { CGFloat($0.height) }
                )
                .overlay {
                    RoundedRectangle(cornerRadius: 4)
                        .strokeBorder(.secondary, lineWidth: 1)
                        .opacity(settings.screen.size == nil ? 0 : 1)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
        }
        .overlay(alignment: settings.handle.alignment) {
            WorkshopMenu(app: app, settings: $settings, switchTo: switchTo, close: close)
                .padding(.trailing, 4)
                .padding(.vertical, 12)
        }
        .preferredColorScheme(settings.appearance.colorScheme)
        // VoiceOver's two-finger scrub goes back to Device Lab once the app
        // itself has nothing left to dismiss.
        .accessibilityAction(.escape, close)
    }
}

/// The workshop button: a small round button on the hosted app's trailing
/// edge, whose menu holds everything Device Lab adds to the app.
private struct WorkshopMenu: View {
    let app: EmbeddedApp
    @Binding var settings: WorkshopSettings
    let switchTo: (EmbeddedApp) -> Void
    let close: () -> Void

    var body: some View {
        Menu {
            Button(action: close) {
                Label("Back to Device Lab", systemImage: "chevron.backward")
            }
            .accessibilityIdentifier("workshop.close")

            // Straight to another app, without the Apps list in between.
            Menu {
                ForEach(EmbeddedApp.allCases.filter { $0 != app }) { other in
                    Button {
                        switchTo(other)
                    } label: {
                        Label(other.name, systemImage: other.systemImage)
                    }
                    .accessibilityIdentifier("workshop.switch.\(other.rawValue)")
                }
            } label: {
                Label("Switch App", systemImage: "square.grid.2x2")
            }
            .accessibilityIdentifier("workshop.switch")

            Section("How \(app.name) Looks") {
                Picker(selection: $settings.appearance) {
                    ForEach(WorkshopSettings.Appearance.allCases) { option in
                        Text(option.name).tag(option)
                    }
                } label: {
                    Label("Appearance", systemImage: "circle.lefthalf.filled")
                }
                .pickerStyle(.menu)
                .accessibilityIdentifier("workshop.appearance")

                Picker(selection: $settings.textSize) {
                    ForEach(WorkshopSettings.TextSize.allCases) { option in
                        Text(option.name).tag(option)
                    }
                } label: {
                    Label("Text Size", systemImage: "textformat.size")
                }
                .pickerStyle(.menu)
                .accessibilityIdentifier("workshop.textSize")

                Picker(selection: $settings.screen) {
                    ForEach(WorkshopSettings.Screen.allCases) { option in
                        Text(option.name).tag(option)
                    }
                } label: {
                    Label("Screen Size", systemImage: "iphone")
                }
                .pickerStyle(.menu)
                .accessibilityIdentifier("workshop.screen")

                if settings.changesPresentation {
                    Button {
                        settings.appearance = .system
                        settings.textSize = .system
                        settings.screen = .full
                    } label: {
                        Label("Show as on This iPhone", systemImage: "arrow.counterclockwise")
                    }
                    .accessibilityIdentifier("workshop.reset")
                }
            }

            Picker(selection: $settings.handle) {
                ForEach(WorkshopSettings.HandlePosition.allCases) { option in
                    Text(option.name).tag(option)
                }
            } label: {
                Label("Move This Button", systemImage: "arrow.up.and.down")
            }
            .pickerStyle(.menu)
            .accessibilityIdentifier("workshop.handle")
        } label: {
            Image(systemName: "wrench.and.screwdriver")
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 44, height: 44)
                .background(Circle().fill(.black.opacity(0.55)))
                .overlay(Circle().strokeBorder(.white.opacity(0.7), lineWidth: 1))
                .contentShape(Circle())
        }
        .accessibilityIdentifier("workshop.menu")
        .accessibilityLabel("Workshop")
        .accessibilityHint("Goes back to Device Lab, switches to another app, or changes how \(app.name) is shown.")
    }
}

private extension WorkshopSettings.Appearance {
    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

private extension WorkshopSettings.TextSize {
    var dynamicTypeSize: DynamicTypeSize? {
        switch self {
        case .system: nil
        case .large: .large
        case .xxxl: .xxxLarge
        case .ax3: .accessibility3
        case .ax5: .accessibility5
        }
    }
}

private extension WorkshopSettings.HandlePosition {
    var alignment: Alignment {
        switch self {
        case .top: .topTrailing
        case .middle: .trailing
        case .bottom: .bottomTrailing
        }
    }
}
