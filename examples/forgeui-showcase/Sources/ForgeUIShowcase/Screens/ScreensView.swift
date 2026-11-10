#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

/// The three example screens ForgeUI 0.1 is judged by: a settings screen, a
/// list, and a create/edit form. Each is built from ForgeUI and plain
/// SwiftUI only, with no styling of its own.
struct ScreensView: View {
    let tasks: TasksModel
    @Binding var theme: ShowcaseTheme
    @Binding var appearance: ShowcaseAppearance
    @Binding var textSize: ShowcaseTextSize

    private enum Screen: Hashable {
        case settings, tasks
    }

    @State private var addingTask = false

    init(tasks: TasksModel, theme: Binding<ShowcaseTheme>, appearance: Binding<ShowcaseAppearance>, textSize: Binding<ShowcaseTextSize>) {
        self.tasks = tasks
        _theme = theme
        _appearance = appearance
        _textSize = textSize
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    NavigationLink(value: Screen.settings) {
                        ForgeRow("Settings", subtitle: "Grouped rows, toggles, pickers", systemImage: "gearshape.fill", tone: .neutral)
                    }
                    .accessibilityIdentifier("screens.settings")
                    NavigationLink(value: Screen.tasks) {
                        ForgeRow("Tasks", subtitle: "Search, badges, swipe actions, states", systemImage: "checklist", tone: .accent)
                    }
                    .accessibilityIdentifier("screens.tasks")
                    Button { addingTask = true } label: {
                        ForgeRow("New Task", subtitle: "A form with validation in a sheet", systemImage: "square.and.pencil", tone: .success)
                    }
                    .foregroundStyle(.primary)
                    .accessibilityIdentifier("screens.newTask")
                } footer: {
                    Text("Built only from ForgeUI and SwiftUI. Change the theme from the paint palette to see every screen follow.")
                }
            }
            .forgeGroupedBackground()
            .navigationTitle("Screens")
            .navigationDestination(for: Screen.self) { screen in
                switch screen {
                case .settings: SettingsScreen(theme: $theme, appearance: $appearance)
                case .tasks: TasksScreen(model: tasks)
                }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    AppearanceMenu(theme: $theme, appearance: $appearance, textSize: $textSize)
                }
            }
            .sheet(isPresented: $addingTask) {
                TaskFormScreen(model: TaskFormModel(), tasks: tasks)
            }
        }
    }
}
#endif
