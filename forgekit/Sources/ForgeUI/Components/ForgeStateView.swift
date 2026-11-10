#if canImport(SwiftUI)
import SwiftUI

/// A button a state view offers, with the identifier UI tests find it by.
public struct ForgeStateAction {
    public let title: LocalizedStringKey
    public let identifier: String
    public let action: () -> Void

    public init(_ title: LocalizedStringKey, identifier: String, action: @escaping () -> Void) {
        self.title = title
        self.identifier = identifier
        self.action = action
    }
}

/// What a screen shows when it has nothing to show yet: no content, still
/// loading, or failed. Built on `ContentUnavailableView`, so it looks like
/// the system's own empty states.
///
///     ForgeStateView.empty("No Tasks", systemImage: "checklist",
///                          description: "Tasks you add appear here.",
///                          action: ForgeStateAction("Add Task", identifier: "tasks.add") { adding = true })
///     ForgeStateView.loading("Loading tasks")
///     ForgeStateView.failed("Couldn't Load Tasks", message: model.loadError,
///                           retry: ForgeStateAction("Try Again", identifier: "tasks.retry") { ... })
public struct ForgeStateView: View {
    private enum Kind {
        case empty(title: LocalizedStringKey, systemImage: String, description: Text?)
        case loading(Text?)
        case failed(title: LocalizedStringKey, message: String?)
    }

    private let kind: Kind
    private let action: ForgeStateAction?

    @Environment(\.forgeTheme) private var theme

    public static func empty(
        _ title: LocalizedStringKey,
        systemImage: String,
        description: LocalizedStringKey? = nil,
        action: ForgeStateAction? = nil
    ) -> ForgeStateView {
        ForgeStateView(kind: .empty(title: title, systemImage: systemImage, description: description.map { Text($0) }), action: action)
    }

    /// No results for a search: the system's own wording, with the query.
    public static func noResults(for query: String) -> ForgeStateView {
        ForgeStateView(kind: .empty(title: "No Results", systemImage: "magnifyingglass", description: Text("Check the spelling or try a new search for “\(query)”.")), action: nil)
    }

    public static func loading(_ label: LocalizedStringKey? = nil) -> ForgeStateView {
        ForgeStateView(kind: .loading(label.map { Text($0) }), action: nil)
    }

    /// `message` is the error as the app describes it (it is shown as written).
    public static func failed(_ title: LocalizedStringKey, message: String? = nil, retry: ForgeStateAction? = nil) -> ForgeStateView {
        ForgeStateView(kind: .failed(title: title, message: message), action: retry)
    }

    public var body: some View {
        switch kind {
        case let .empty(title, systemImage, description):
            ContentUnavailableView {
                Label(title, systemImage: systemImage)
            } description: {
                if let description { description }
            } actions: {
                actionButton(.forgePrimary)
            }
        case let .loading(label):
            VStack(spacing: theme.spacing.m) {
                ProgressView()
                    .controlSize(.large)
                if let label {
                    label
                        .font(.subheadline)
                        .foregroundStyle(theme.colors.textSecondary)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityElement(children: .combine)
        case let .failed(title, message):
            ContentUnavailableView {
                Label {
                    Text(title)
                } icon: {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(theme.colors.tone(.warning).solid)
                }
            } description: {
                if let message { Text(message) }
            } actions: {
                actionButton(.forgeSecondary)
            }
        }
    }

    @ViewBuilder
    private func actionButton(_ style: ForgeButtonStyle) -> some View {
        if let action {
            Button(action: action.action) { Text(action.title) }
                .buttonStyle(style)
                .accessibilityIdentifier(action.identifier)
        }
    }
}
#endif
