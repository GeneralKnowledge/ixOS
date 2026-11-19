#if canImport(SwiftUI)
import SwiftUI

/// A button a state view offers, with the identifier UI tests find it by.
/// It is either a button or, when there is a choice to make (which kind of
/// chat to start, how to add a book), a menu of buttons.
public struct ForgeStateAction {
    public let title: LocalizedStringKey
    public let identifier: String
    public let action: () -> Void
    let systemImage: String?
    let menu: AnyView?

    public init(_ title: LocalizedStringKey, identifier: String, action: @escaping () -> Void) {
        self.init(title: title, identifier: identifier, action: action, systemImage: nil, menu: nil)
    }

    private init(title: LocalizedStringKey, identifier: String, action: @escaping () -> Void, systemImage: String?, menu: AnyView?) {
        self.title = title
        self.identifier = identifier
        self.action = action
        self.systemImage = systemImage
        self.menu = menu
    }

    /// A menu of choices. The buttons in `content` carry their own
    /// identifiers; `identifier` is the menu's.
    ///
    ///     ForgeStateAction.menu("Add Book", systemImage: "plus", identifier: "library.empty.add") {
    ///         Button("Enter Details") { … }.accessibilityIdentifier("library.add.manual")
    ///     }
    public static func menu<Content: View>(
        _ title: LocalizedStringKey,
        systemImage: String? = nil,
        identifier: String,
        @ViewBuilder content: () -> Content
    ) -> ForgeStateAction {
        ForgeStateAction(title: title, identifier: identifier, action: {}, systemImage: systemImage, menu: AnyView(content()))
    }
}

/// What a screen shows when it has nothing to show: no content yet, no
/// results for a search, still loading, or failed. Built on
/// `ContentUnavailableView`, so it looks like the system's own empty states.
///
///     ForgeStateView.empty("No Tasks", systemImage: "checklist",
///                          description: "Tasks you add appear here.",
///                          action: ForgeStateAction("Add Task", identifier: "tasks.add") { adding = true })
///     ForgeStateView.noResults(for: model.searchText)
///     ForgeStateView.loading("Loading tasks")
///     ForgeStateView.failed("Couldn't Load Tasks", message: model.loadError,
///                           retry: ForgeStateAction("Try Again", identifier: "tasks.retry") { ... })
///
/// Keep the states apart: show `loading` until the model has loaded (never
/// an empty list that is still loading), `noResults` only for a search, and
/// `failed` with a retry that repeats the operation that failed. A deleted
/// or missing item is an `empty` state with no action.
public struct ForgeStateView: View {
    private enum Kind {
        case empty(title: Text, systemImage: String, description: Text?)
        case noResults(query: String, advice: Text?)
        case loading(Text?)
        case failed(title: Text, message: String?)
    }

    private let kind: Kind
    private let action: ForgeStateAction?
    private let secondary: ForgeStateAction?

    @Environment(\.forgeTheme) private var theme

    private init(kind: Kind, action: ForgeStateAction? = nil, secondary: ForgeStateAction? = nil) {
        self.kind = kind
        self.action = action
        self.secondary = secondary
    }

    /// Nothing here yet, or the item is gone. `action` is the main way to
    /// fill the screen; `secondary` an alternative.
    public static func empty(
        _ title: LocalizedStringKey,
        systemImage: String,
        description: LocalizedStringKey? = nil,
        action: ForgeStateAction? = nil,
        secondary: ForgeStateAction? = nil
    ) -> ForgeStateView {
        ForgeStateView(
            kind: .empty(title: Text(title), systemImage: systemImage, description: description.map { Text($0) }),
            action: action, secondary: secondary)
    }

    /// An empty state whose title or description comes from the app's data
    /// (a place's name, a reason from Rust). Both are shown as written.
    @_disfavoredOverload
    public static func empty<Title: StringProtocol>(
        _ title: Title,
        systemImage: String,
        message: String? = nil,
        action: ForgeStateAction? = nil,
        secondary: ForgeStateAction? = nil
    ) -> ForgeStateView {
        ForgeStateView(
            kind: .empty(title: Text(title), systemImage: systemImage, description: message.map { Text(verbatim: $0) }),
            action: action, secondary: secondary)
    }

    /// No results for a search: the system's own state, in the system's
    /// words and language, with the query when there is one. A search with
    /// its own advice or a way forward (enter the details by hand) passes
    /// `advice` and `action`; it then keeps the system's title and symbol.
    public static func noResults(
        for query: String = "",
        advice: LocalizedStringKey? = nil,
        action: ForgeStateAction? = nil
    ) -> ForgeStateView {
        ForgeStateView(kind: .noResults(query: query, advice: advice.map { Text($0) }), action: action)
    }

    public static func loading(_ label: LocalizedStringKey? = nil) -> ForgeStateView {
        ForgeStateView(kind: .loading(label.map { Text($0) }))
    }

    /// `message` is the error as the app describes it (it is shown as
    /// written). `retry` repeats the operation that failed; `secondary` is
    /// another way forward (enter details by hand, choose another place).
    public static func failed(
        _ title: LocalizedStringKey,
        message: String? = nil,
        retry: ForgeStateAction? = nil,
        secondary: ForgeStateAction? = nil
    ) -> ForgeStateView {
        ForgeStateView(kind: .failed(title: Text(title), message: message), action: retry, secondary: secondary)
    }

    /// A failure whose title comes from the app's data, shown as written.
    @_disfavoredOverload
    public static func failed<Title: StringProtocol>(
        _ title: Title,
        message: String? = nil,
        retry: ForgeStateAction? = nil,
        secondary: ForgeStateAction? = nil
    ) -> ForgeStateView {
        ForgeStateView(kind: .failed(title: Text(title), message: message), action: retry, secondary: secondary)
    }

    public var body: some View {
        switch kind {
        case let .empty(title, systemImage, description):
            ContentUnavailableView {
                Label { title } icon: { Image(systemName: systemImage) }
            } description: {
                if let description { description }
            } actions: {
                actionButton(action, .forgePrimary)
                actionButton(secondary, .forgeTertiary)
            }
        case let .noResults(query, advice):
            if advice != nil || action != nil {
                ContentUnavailableView {
                    Label("No Results", systemImage: "magnifyingglass")
                } description: {
                    if let advice { advice }
                } actions: {
                    actionButton(action, .forgeTertiary)
                }
            } else if query.isEmpty {
                ContentUnavailableView.search
            } else {
                ContentUnavailableView.search(text: query)
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
                    title
                } icon: {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(theme.colors.tone(.warning).solid)
                }
            } description: {
                if let message { Text(verbatim: message) }
            } actions: {
                actionButton(action, .forgeSecondary)
                actionButton(secondary, .forgeTertiary)
            }
        }
    }

    @ViewBuilder
    private func menuLabel(_ action: ForgeStateAction) -> some View {
        if let systemImage = action.systemImage {
            Label(action.title, systemImage: systemImage)
                .labelStyle(.titleAndIcon)
        } else {
            Text(action.title)
        }
    }

    @ViewBuilder
    private func actionButton(_ action: ForgeStateAction?, _ style: ForgeButtonStyle) -> some View {
        if let action {
            if let menu = action.menu {
                Menu {
                    menu
                } label: {
                    // Drawn by ForgeUI like the state's plain buttons, so a
                    // menu looks the same as a button beside it. The system's
                    // prominent style on a menu left the icon in the accent
                    // colour on an accent pill (invisible, but taking space)
                    // and a paler pill (Relay, iOS 26.3, 2026-11-18).
                    ForgeButtonChrome(label: menuLabel(action), kind: style.kind, expands: style.expands, isPressed: false)
                }
                .menuStyle(.button)
                .buttonStyle(.plain)
                .accessibilityIdentifier(action.identifier)
            } else {
                Button(action: action.action) { Text(action.title) }
                    .buttonStyle(style)
                    .accessibilityIdentifier(action.identifier)
            }
        }
    }
}
#endif
