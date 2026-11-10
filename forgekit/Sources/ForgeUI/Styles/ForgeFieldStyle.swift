#if canImport(SwiftUI)
import SwiftUI

/// A bordered field for text entry outside a `Form` (inside a `Form`, the
/// row is already the field's frame, so leave the default style). Works for
/// `TextField` and `SecureField`:
///
///     TextField("Email", text: $email)
///         .textFieldStyle(.forge)
public struct ForgeTextFieldStyle: TextFieldStyle {
    public init() {}

    // swiftlint:disable:next identifier_name
    public func _body(configuration: TextField<Self._Label>) -> some View {
        ForgeTextFieldChrome { configuration }
    }
}

extension TextFieldStyle where Self == ForgeTextFieldStyle {
    public static var forge: ForgeTextFieldStyle { ForgeTextFieldStyle() }
}

private struct ForgeTextFieldChrome<Content: View>: View {
    @ViewBuilder let content: () -> Content

    @Environment(\.forgeTheme) private var theme
    @Environment(\.isEnabled) private var isEnabled
    @ScaledMetric(relativeTo: .body) private var minHeight: CGFloat = 44

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: theme.radius.m, style: .continuous)
        content()
            .font(.body)
            .foregroundStyle(theme.colors.textPrimary)
            .padding(.horizontal, theme.spacing.m)
            .frame(minHeight: minHeight)
            .background(theme.colors.surface, in: shape)
            .overlay(shape.strokeBorder(theme.colors.separator, lineWidth: 1))
            .opacity(isEnabled ? 1 : 0.5)
    }
}

/// A labelled form field with an optional help or validation line under it.
/// The field itself is any SwiftUI control:
///
///     ForgeField("Title", message: model.titleMessage) {
///         TextField("What needs doing?", text: $model.title)
///             .accessibilityIdentifier("task.title")
///     }
///
/// Inside a `Form` it sits in the row; outside one, give the field
/// `.textFieldStyle(.forge)`. `message` usually comes from the view model,
/// for example from a Rust validation result (`ForgeFieldMessage.error(...)`).
public struct ForgeField<Content: View>: View {
    private let label: Text
    private let message: ForgeFieldMessage?
    private let content: Content

    @Environment(\.forgeTheme) private var theme

    public init(_ label: LocalizedStringKey, message: ForgeFieldMessage? = nil, @ViewBuilder content: () -> Content) {
        self.label = Text(label)
        self.message = message
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: theme.spacing.xs) {
            label
                .font(.subheadline.weight(.medium))
                .foregroundStyle(theme.colors.textSecondary)
                .accessibilityHidden(true)
            content
                .accessibilityLabel(label)
            if let message {
                // Read by VoiceOver right after the field.
                ForgeFieldMessageView(message: message)
            }
        }
        .padding(.vertical, theme.spacing.xxs)
    }
}

/// A help or validation line on its own, for layouts `ForgeField` doesn't fit.
public struct ForgeFieldMessageView: View {
    let message: ForgeFieldMessage

    @Environment(\.forgeTheme) private var theme

    public init(message: ForgeFieldMessage) {
        self.message = message
    }

    private var symbol: String? {
        switch message.kind {
        case .help: nil
        case .success: "checkmark.circle.fill"
        case .warning: "exclamationmark.triangle.fill"
        case .error: "exclamationmark.circle.fill"
        }
    }

    public var body: some View {
        let color = message.kind == .help ? theme.colors.textSecondary : theme.colors.tone(message.tone).text
        HStack(alignment: .firstTextBaseline, spacing: theme.spacing.xs) {
            if let symbol {
                Image(systemName: symbol)
                    .accessibilityHidden(true)
            }
            Text(message.text)
                .fixedSize(horizontal: false, vertical: true)
        }
        .font(.footnote)
        .foregroundStyle(color)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(message.isError ? Text("Error: ") + Text(message.text) : Text(message.text))
    }
}
#endif
