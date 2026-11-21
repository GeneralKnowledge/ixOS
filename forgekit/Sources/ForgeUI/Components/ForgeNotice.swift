#if canImport(SwiftUI)
import SwiftUI

/// Something is wrong, but the screen still works: say what, what still
/// works, and offer at most one action.
///
///     ForgeNotice("Offline. Messages you send now go out when you're back.",
///                 systemImage: "wifi.slash",
///                 action: ForgeStateAction("Reconnect", identifier: "chat.reconnect") { reconnect() })
///     ForgeNotice("The map can't be updated right now. Recording carries on.", systemImage: "map")
///
/// It sits inside the screen it is about: at the top of a list, above a
/// card, or in a `.safeAreaInset(edge: .top)`. In a `List`, give it a row of
/// its own with `.listRowInsets(EdgeInsets())` and
/// `.listRowBackground(Color.clear)`. When the screen has nothing to show at
/// all, use `ForgeStateView.failed` instead.
///
/// The tone defaults to `.warning`; use `.info` for news that needs no
/// action ("Connecting…") and `.danger` only when something was lost. An
/// app with its own surfaces (Weather's sky cards) hides the tinted
/// background with `.forgeNoticeBackground(.hidden)`, and the notice then
/// draws in the colours around it.
public struct ForgeNotice: View {
    private let message: Text
    private let systemImage: String
    private let tone: ForgeTone
    private let action: ForgeStateAction?

    @Environment(\.forgeTheme) private var theme
    @Environment(\.forgeNoticeBackground) private var background

    public init(
        _ message: LocalizedStringKey,
        systemImage: String,
        tone: ForgeTone = .warning,
        action: ForgeStateAction? = nil
    ) {
        self.message = Text(message)
        self.systemImage = systemImage
        self.tone = tone
        self.action = action
    }

    /// A message from the app's data (a reason from Rust), shown as written.
    @_disfavoredOverload
    public init<S: StringProtocol>(
        _ message: S,
        systemImage: String,
        tone: ForgeTone = .warning,
        action: ForgeStateAction? = nil
    ) {
        self.message = Text(message)
        self.systemImage = systemImage
        self.tone = tone
        self.action = action
    }

    public var body: some View {
        let tinted = background != .hidden
        let colors = theme.colors.tone(tone)
        let shape = RoundedRectangle(cornerRadius: theme.radius.m, style: .continuous)
        HStack(alignment: .firstTextBaseline, spacing: theme.spacing.s) {
            Image(systemName: systemImage)
                .foregroundStyle(tinted ? AnyShapeStyle(colors.text) : AnyShapeStyle(.primary))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: theme.spacing.xs) {
                message
                    .foregroundStyle(tinted ? AnyShapeStyle(theme.colors.textPrimary) : AnyShapeStyle(.primary))
                    .fixedSize(horizontal: false, vertical: true)
                if let action {
                    Button(action: action.action) { Text(action.title) }
                        .buttonStyle(.plain)
                        .fontWeight(.semibold)
                        .foregroundStyle(tinted ? AnyShapeStyle(colors.text) : AnyShapeStyle(.primary))
                        .accessibilityIdentifier(action.identifier)
                }
            }
            Spacer(minLength: 0)
        }
        .font(.footnote)
        .padding(.horizontal, theme.spacing.m)
        .padding(.vertical, theme.spacing.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
            if tinted {
                shape.fill(colors.soft)
            }
        }
        // Read as one sentence, but keep the action a button of its own.
        .accessibilityElement(children: action == nil ? .combine : .contain)
    }
}

extension View {
    /// Hides the tinted background of the `ForgeNotice`s inside this view,
    /// for an app that draws them on its own surfaces.
    public func forgeNoticeBackground(_ visibility: Visibility) -> some View {
        environment(\.forgeNoticeBackground, visibility)
    }
}

private struct ForgeNoticeBackgroundKey: EnvironmentKey {
    static let defaultValue = Visibility.automatic
}

extension EnvironmentValues {
    var forgeNoticeBackground: Visibility {
        get { self[ForgeNoticeBackgroundKey.self] }
        set { self[ForgeNoticeBackgroundKey.self] = newValue }
    }
}
#endif
