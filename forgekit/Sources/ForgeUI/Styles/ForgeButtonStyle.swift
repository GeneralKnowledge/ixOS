#if canImport(SwiftUI)
import SwiftUI

/// ForgeUI's button styles, applied to SwiftUI's own `Button`:
///
///     Button("Save", action: save)
///         .buttonStyle(.forgePrimary)
///         .accessibilityIdentifier("task.save")
///
/// Disabled and pressed states come from SwiftUI (`.disabled(...)`), and the
/// hit area is at least 44 points tall. `.controlSize(.small)` makes a
/// compact button; `.forge(.primary, expands: true)` fills the width.
public struct ForgeButtonStyle: ButtonStyle {
    public enum Kind: String, CaseIterable, Identifiable, Hashable, Sendable {
        public var id: String { rawValue }

        /// The one main action on a screen: a filled accent button.
        case primary
        /// Other actions: tinted, quieter.
        case secondary
        /// Low-emphasis actions: accent text only.
        case tertiary
        /// Deletes or discards something: a filled danger button.
        case destructive
    }

    public var kind: Kind
    public var expands: Bool

    public init(_ kind: Kind, expands: Bool = false) {
        self.kind = kind
        self.expands = expands
    }

    public func makeBody(configuration: Configuration) -> some View {
        ForgeButtonBody(configuration: configuration, kind: kind, expands: expands)
    }
}

extension ButtonStyle where Self == ForgeButtonStyle {
    public static var forgePrimary: ForgeButtonStyle { ForgeButtonStyle(.primary) }
    public static var forgeSecondary: ForgeButtonStyle { ForgeButtonStyle(.secondary) }
    public static var forgeTertiary: ForgeButtonStyle { ForgeButtonStyle(.tertiary) }
    public static var forgeDestructive: ForgeButtonStyle { ForgeButtonStyle(.destructive) }

    public static func forge(_ kind: ForgeButtonStyle.Kind, expands: Bool = false) -> ForgeButtonStyle {
        ForgeButtonStyle(kind, expands: expands)
    }
}

private struct ForgeButtonBody: View {
    let configuration: ButtonStyleConfiguration
    let kind: ForgeButtonStyle.Kind
    let expands: Bool

    @Environment(\.forgeTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.controlSize) private var controlSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ScaledMetric(relativeTo: .headline) private var minHeight: CGFloat = 44
    @ScaledMetric(relativeTo: .subheadline) private var compactHeight: CGFloat = 32

    private var compact: Bool { controlSize == .small || controlSize == .mini }

    private var foreground: Color {
        switch kind {
        case .primary: theme.colors.accent.onSolid
        case .secondary, .tertiary: theme.colors.accent.text
        case .destructive: theme.colors.tone(.danger).onSolid
        }
    }

    private var fill: Color {
        switch kind {
        case .primary: theme.colors.accent.solid
        case .secondary: theme.colors.accent.soft
        case .tertiary: .clear
        case .destructive: theme.colors.tone(.danger).solid
        }
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: theme.radius.m, style: .continuous)
        configuration.label
            .font(compact ? .subheadline.weight(.semibold) : .headline)
            .foregroundStyle(foreground)
            .tint(foreground)
            .lineLimit(2)
            .multilineTextAlignment(.center)
            .padding(.horizontal, compact ? theme.spacing.m : theme.spacing.l)
            .padding(.vertical, theme.spacing.s)
            .frame(maxWidth: expands ? .infinity : nil, minHeight: compact ? compactHeight : minHeight)
            .background(fill, in: shape)
            .contentShape(shape)
            .opacity(isEnabled ? (configuration.isPressed ? 0.7 : 1) : 0.4)
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.98 : 1)
            .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: configuration.isPressed)
    }
}

/// A round, icon-only button: `Button { } label: { Label("Add", systemImage: "plus") }`
/// with `.buttonStyle(.forgeIcon)`. The label's title stays as the
/// VoiceOver label; only the symbol is drawn.
public struct ForgeIconButtonStyle: ButtonStyle {
    public var tone: ForgeTone

    public init(tone: ForgeTone = .accent) {
        self.tone = tone
    }

    public func makeBody(configuration: Configuration) -> some View {
        ForgeIconButtonBody(configuration: configuration, tone: tone)
    }
}

extension ButtonStyle where Self == ForgeIconButtonStyle {
    public static var forgeIcon: ForgeIconButtonStyle { ForgeIconButtonStyle() }

    public static func forgeIcon(tone: ForgeTone) -> ForgeIconButtonStyle {
        ForgeIconButtonStyle(tone: tone)
    }
}

private struct ForgeIconButtonBody: View {
    let configuration: ButtonStyleConfiguration
    let tone: ForgeTone

    @Environment(\.forgeTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 44

    var body: some View {
        let colors = theme.colors.tone(tone)
        configuration.label
            .labelStyle(.iconOnly)
            .font(.body.weight(.semibold))
            .foregroundStyle(colors.text)
            .frame(width: size, height: size)
            .background(colors.soft, in: Circle())
            .contentShape(Circle())
            .opacity(isEnabled ? (configuration.isPressed ? 0.7 : 1) : 0.4)
    }
}

/// A button that shows a spinner in place of its title while `isWorking`,
/// and ignores taps until it finishes. Style it like any button:
///
///     ForgeButton("Save", isWorking: model.isSaving) { Task { await model.save() } }
///         .buttonStyle(.forge(.primary, expands: true))
///         .accessibilityIdentifier("task.save")
public struct ForgeButton: View {
    private let title: Text
    private let systemImage: String?
    private let isWorking: Bool
    private let role: ButtonRole?
    private let action: () -> Void

    public init(
        _ title: LocalizedStringKey,
        systemImage: String? = nil,
        role: ButtonRole? = nil,
        isWorking: Bool = false,
        action: @escaping () -> Void
    ) {
        self.title = Text(title)
        self.systemImage = systemImage
        self.role = role
        self.isWorking = isWorking
        self.action = action
    }

    public var body: some View {
        Button(role: role) {
            if !isWorking { action() }
        } label: {
            ZStack {
                Group {
                    if let systemImage {
                        Label { title } icon: { Image(systemName: systemImage) }
                    } else {
                        title
                    }
                }
                .opacity(isWorking ? 0 : 1)
                if isWorking {
                    ProgressView()
                }
            }
        }
        .accessibilityValue(isWorking ? Text("In progress") : Text(""))
    }
}
#endif
