#if canImport(SwiftUI)
import SwiftUI

/// The root view for a ForgeHost: a spinner while the core opens, the app
/// once it is ready, a retry screen if opening failed.
public struct ForgeRoot<Core: ForgeCore, Content: View>: View {
    @Bindable private var host: ForgeHost<Core>
    private let content: (Core) -> Content

    public init(_ host: ForgeHost<Core>, @ViewBuilder content: @escaping (Core) -> Content) {
        self.host = host
        self.content = content
    }

    public var body: some View {
        Group {
            switch host.status {
            case .loading:
                ProgressView()
            case .ready:
                if let core = host.core { content(core) }
            case .closed:
                Color.clear
            case .failed(let message):
                ContentUnavailableView {
                    Label("Can't Open", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(message)
                } actions: {
                    Button("Try Again") { Task { await host.start() } }
                        .accessibilityIdentifier("forge.retry")
                }
            }
        }
        .task { await host.start() }
    }
}
#endif
