/// What a `ForgeButton` allows while its work runs. Plain values, so the
/// rules are checked on Linux; `ForgeButton` draws them.
struct ForgeButtonState: Hashable, Sendable {
    /// Work is running: the app said so (`isWorking:`), or the button's
    /// own async action hasn't returned.
    var isWorking: Bool
    /// The spinner is up. With `isWorking:` it shows at once; for the
    /// button's own async action only after `spinnerDelay`, so a fast
    /// local write never flickers and a slow one always shows.
    var showsSpinner: Bool

    init(isWorking: Bool, showsSpinner: Bool? = nil) {
        self.isWorking = isWorking
        self.showsSpinner = isWorking && (showsSpinner ?? true)
    }

    /// How long an async action runs before its spinner appears.
    static let spinnerDelay: Duration = .milliseconds(400)

    /// A tap runs the action only when the button isn't already working.
    var runsAction: Bool { !isWorking }
    /// While working the button is disabled, so VoiceOver reads it as dimmed
    /// and Voice Control, Full Keyboard Access and a pointer can't press it
    /// again, the same as a button the app disabled itself.
    var isDisabled: Bool { isWorking }
    /// It keeps its full colour while working: the spinner says it is busy,
    /// and a faded spinner is hard to see. A button the app disabled fades.
    var fadesWhenDisabled: Bool { !isWorking }
    /// VoiceOver hears "In progress" only while the spinner shows.
    var announcesProgress: Bool { showsSpinner }
}

/// Which of a ForgeUI button's looks apply: pressed, disabled, working,
/// compact, Reduce Motion. Plain values, so the rules are checked on Linux;
/// `forgeButtonLook` draws them, and every ForgeUI button is drawn by it.
struct ForgeButtonLook: Hashable, Sendable {
    var isEnabled: Bool
    var isWorking: Bool
    var isPressed: Bool
    var isCompact: Bool
    var reduceMotion: Bool

    /// A disabled button fades; a working one keeps its colour (its
    /// spinner says it is busy). A press dims it a little.
    var opacity: Double {
        guard isEnabled || isWorking else { return 0.4 }
        return isPressed ? 0.7 : 1
    }

    /// A press shrinks it slightly, unless Reduce Motion is on.
    var scale: Double { isPressed && !reduceMotion ? 0.98 : 1 }
    var animatesPress: Bool { !reduceMotion }
}

#if canImport(SwiftUI)
import SwiftUI

private struct ForgeButtonIsWorkingKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// Set by `ForgeButton` while its work runs, so ForgeUI's button styles
    /// draw it at full colour although it is disabled.
    var forgeButtonIsWorking: Bool {
        get { self[ForgeButtonIsWorkingKey.self] }
        set { self[ForgeButtonIsWorkingKey.self] = newValue }
    }
}

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
        configuration.label.forgeButtonLook(kind, expands: expands, isPressed: configuration.isPressed)
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

extension View {
    /// ForgeUI's button look on any view, for controls that aren't a
    /// `Button` (a `Menu`'s label, a renderer's own control). It is the one
    /// implementation: `ForgeButtonStyle` draws its buttons with it. Pass
    /// `isPressed` from your control; disabled comes from `.disabled(...)`,
    /// compact from `.controlSize(.small)`, working from `ForgeButton`, and
    /// Reduce Motion and Dynamic Type from the system, as for the style.
    ///
    ///     Menu { … } label: {
    ///         Label("New", systemImage: "plus").forgeButtonLook(.primary)
    ///     }
    public func forgeButtonLook(
        _ kind: ForgeButtonStyle.Kind, expands: Bool = false, isPressed: Bool = false
    ) -> some View {
        ForgeButtonChrome(label: self, kind: kind, expands: expands, isPressed: isPressed)
    }
}

/// What a ForgeUI button looks like, for any label (`forgeButtonLook`).
private struct ForgeButtonChrome<Content: View>: View {
    let label: Content
    let kind: ForgeButtonStyle.Kind
    let expands: Bool
    let isPressed: Bool

    @Environment(\.forgeTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.forgeButtonIsWorking) private var isWorking
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
        let look = ForgeButtonLook(
            isEnabled: isEnabled, isWorking: isWorking, isPressed: isPressed,
            isCompact: compact, reduceMotion: reduceMotion
        )
        return label
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
            .opacity(look.opacity)
            .scaleEffect(look.scale)
            .animation(look.animatesPress ? .easeOut(duration: 0.12) : nil, value: isPressed)
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
    @Environment(\.forgeButtonIsWorking) private var isWorking
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 44

    var body: some View {
        let colors = theme.colors.tone(tone)
        let look = ForgeButtonLook(
            isEnabled: isEnabled, isWorking: isWorking, isPressed: configuration.isPressed,
            isCompact: false, reduceMotion: reduceMotion
        )
        return configuration.label
            .labelStyle(.iconOnly)
            .font(.body.weight(.semibold))
            .foregroundStyle(colors.text)
            .frame(width: size, height: size)
            .background(colors.soft, in: Circle())
            .contentShape(Circle())
            .opacity(look.opacity)
    }
}

/// A button that shows a spinner in place of its title while its work runs,
/// and is disabled until it finishes: taps, VoiceOver, Voice Control and
/// the keyboard can't press it again, and VoiceOver reads "In progress".
/// ForgeUI's styles keep it at full colour while it works.
///
/// Give it an async action and it tracks the work itself. The spinner
/// appears only if the work takes longer than 0.4 s, so a fast local write
/// doesn't flicker and a slow one is never silent:
///
///     ForgeButton("Trust Again") { await model.trustAgain(device) }
///         .accessibilityIdentifier("device.trustAgain")
///
/// For work the model tracks itself, pass `isWorking:`; the spinner shows
/// at once. Style it like any button:
///
///     ForgeButton("Save", isWorking: model.isSaving) { Task { await model.save() } }
///         .buttonStyle(.forge(.primary, expands: true))
///         .accessibilityIdentifier("task.save")
public struct ForgeButton: View {
    private let title: Text
    private let systemImage: String?
    private let isWorking: Bool
    private let role: ButtonRole?
    private let action: Action

    private enum Action {
        case plain(() -> Void)
        case work(@MainActor () async -> Void)
    }

    /// The button's own async work: running, and whether its spinner is due.
    @State private var running = false
    @State private var spinnerDue = false

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
        self.action = .plain(action)
    }

    /// A button that runs async work and shows it: disabled at once, with a
    /// spinner if the work takes longer than 0.4 s.
    public init(
        _ title: LocalizedStringKey,
        systemImage: String? = nil,
        role: ButtonRole? = nil,
        action: @escaping @MainActor () async -> Void
    ) {
        self.title = Text(title)
        self.systemImage = systemImage
        self.role = role
        self.isWorking = false
        self.action = .work(action)
    }

    private var state: ForgeButtonState {
        ForgeButtonState(isWorking: isWorking || running, showsSpinner: isWorking || spinnerDue)
    }

    public var body: some View {
        let state = state
        return Button(role: role) {
            guard state.runsAction else { return }
            switch action {
            case .plain(let run): run()
            case .work(let run): start(run)
            }
        } label: {
            ZStack {
                Group {
                    if let systemImage {
                        Label { title } icon: { Image(systemName: systemImage) }
                    } else {
                        title
                    }
                }
                .opacity(state.showsSpinner ? 0 : 1)
                if state.showsSpinner {
                    ProgressView()
                }
            }
        }
        .disabled(state.isDisabled)
        .environment(\.forgeButtonIsWorking, !state.fadesWhenDisabled)
        .accessibilityValue(state.announcesProgress ? Text("In progress") : Text(""))
    }

    private func start(_ run: @escaping @MainActor () async -> Void) {
        running = true
        Task { @MainActor in
            let spinner = Task { @MainActor in
                try? await Task.sleep(for: ForgeButtonState.spinnerDelay)
                if !Task.isCancelled { spinnerDue = true }
            }
            await run()
            spinner.cancel()
            spinnerDue = false
            running = false
        }
    }
}
#endif
