@preconcurrency import ForgeFFI
import ForgeUI
import SwiftUI

/// The app's root. It owns the model, opens the Rust core once, and shows
/// onboarding, the tabs, a spinner while opening, or a recovery screen.
public struct ContentView: View {
    @State private var model = GoodDogModel()

    public init() {}

    public var body: some View {
        Group {
            switch model.phase {
            case .loading:
                ProgressView()
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
        .forgeTheme(.standard)
        .task { await model.open() }
        .forgeLifecycle { model.setPhase($0) }
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
        .termSheet(model: model)
    }
}
