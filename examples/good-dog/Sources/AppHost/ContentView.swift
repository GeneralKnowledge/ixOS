@preconcurrency import GoodDogFFI
import ForgeAppKit
import ForgeKit
import ForgeUI
import SwiftUI

/// The app's root. It owns the model over the process's host, opens the
/// Rust core once, and shows onboarding, the tabs, a spinner while opening,
/// or a recovery screen. The App attaches the host to the lifecycle.
public struct ContentView: View {
    @State private var model: GoodDogModel

    public init(host: ForgeHost<AppCore>) {
        _model = State(initialValue: GoodDogModel(host: host))
    }

    public var body: some View {
        Group {
            switch model.phase {
            case .loading:
                ForgeStateView.loading()
            case .needsDog:
                OnboardingView(model: model)
            case .ready:
                MainTabs(model: model)
            case .failed(let message):
                ForgeStateView.failed(
                    "Can't Open Good Dog",
                    message: message,
                    retry: ForgeStateAction("Try Again", identifier: "content.retry") {
                        Task { await model.retry() }
                    }
                )
            }
        }
        .forgeTheme(ForgeTheme(accent: .green))
        .task { await model.open() }
        // A commit, missed events or a return from the background.
        .onChange(of: model.host.changes) { _, _ in
            Task { await model.hostChanged() }
        }
        // Back from the background: the reminders' week moves on.
        .onChange(of: model.host.phase) { old, new in
            if old == .background, new == .active || new == .inactive {
                Task { await model.resumed() }
            }
        }
    }
}

struct MainTabs: View {
    let model: GoodDogModel

    var body: some View {
        TabView {
            TodayView(model: model)
                .tabItem {
                    Label("Today", systemImage: "sun.max")
                        .accessibilityIdentifier("tab.today")
                }
            LearnView(model: model)
                .tabItem {
                    Label("Learn", systemImage: "book")
                        .accessibilityIdentifier("tab.learn")
                }
            MyDogView(model: model)
                .tabItem {
                    Label("My Dog", systemImage: "pawprint")
                        .accessibilityIdentifier("tab.dog")
                }
        }
        // Once, over every tab: a failure on a pushed screen (a goal's path)
        // showed behind it when the alert sat on each tab's first screen.
        .forgeAlert(failure: Bindable(model).actionError)
        .termSheet(model: model)
        .stageCelebration(model: model)
    }
}
