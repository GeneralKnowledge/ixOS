#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

/// Draws a `ForgeLoad`: a spinner while loading, ForgeUI's failed state
/// (with Try Again when the failure is retryable and `retry` is given),
/// and `content` once loaded. `idle` shows `idle`, empty by default.
///
///     ForgeLoadView(model.results, loading: "Searching", retry: ForgeStateAction("Try Again", identifier: "search.retry") { … }) { books in
///         List(books) { … }
///     }
public struct ForgeLoadView<Value, Content: View, Idle: View>: View {
    private let load: ForgeLoad<Value>
    private let loadingLabel: LocalizedStringKey?
    private let retry: ForgeStateAction?
    private let content: (Value) -> Content
    private let idle: () -> Idle

    public init(
        _ load: ForgeLoad<Value>,
        loading loadingLabel: LocalizedStringKey? = nil,
        retry: ForgeStateAction? = nil,
        @ViewBuilder content: @escaping (Value) -> Content,
        @ViewBuilder idle: @escaping () -> Idle
    ) {
        self.load = load
        self.loadingLabel = loadingLabel
        self.retry = retry
        self.content = content
        self.idle = idle
    }

    public var body: some View {
        switch load {
        case .idle:
            idle()
        case .loading:
            ForgeStateView.loading(loadingLabel)
        case .loaded(let value):
            content(value)
        case .failed(let failure):
            ForgeStateView.failed(
                LocalizedStringKey(failure.title),
                message: failure.message,
                retry: failure.isRetryable ? retry : nil
            )
        }
    }
}

extension ForgeLoadView where Idle == EmptyView {
    public init(
        _ load: ForgeLoad<Value>,
        loading loadingLabel: LocalizedStringKey? = nil,
        retry: ForgeStateAction? = nil,
        @ViewBuilder content: @escaping (Value) -> Content
    ) {
        self.init(load, loading: loadingLabel, retry: retry, content: content, idle: { EmptyView() })
    }
}
#endif
