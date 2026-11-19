#if canImport(SwiftUI)
import ForgeUI
import SwiftUI

extension View {
    /// Shows the model's banner at the bottom of this view, above the home
    /// indicator. It floats with ForgeUI's `forgeBar`, which gives every
    /// scroll view below (pushed screens too) room for it, so it never
    /// covers a list's last row. Its button's action goes to `perform`;
    /// the banner goes after its duration, when its button is tapped, or
    /// when it is swiped down. VoiceOver announces it.
    ///
    ///     .forgeBanner($model.banner) { action in Task { await model.undo(action) } }
    public func forgeBanner<Action>(
        _ banner: Binding<ForgeBanner<Action>?>,
        perform: @escaping (Action) -> Void
    ) -> some View {
        modifier(ForgeBannerModifier(banner: banner, perform: perform))
    }

    /// Shows a banner that only informs.
    public func forgeBanner(_ banner: Binding<ForgeBanner<Never>?>) -> some View {
        modifier(ForgeBannerModifier(banner: banner, perform: { _ in }))
    }
}

private struct ForgeBannerModifier<Action>: ViewModifier {
    @Binding var banner: ForgeBanner<Action>?
    let perform: (Action) -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.accessibilityVoiceOverEnabled) private var voiceOver

    func body(content: Content) -> some View {
        content
            // A ForgeUI bar: every scroll view under it, on the stack's
            // pushed screens too, gets room to scroll its last row above
            // the banner. A safe-area inset on the stack didn't reach a
            // pushed list (Commonplace's timeline, seen on the phone
            // 2026-11-18). Inside a TabView, attach it to each tab's
            // content so it sits above the tab bar.
            .forgeBar {
                if let shown = banner {
                    ForgeBannerView(banner: shown) {
                        if let button = shown.button {
                            banner = nil
                            perform(button.action)
                        }
                    } dismiss: {
                        banner = nil
                    }
                    .transition(reduceMotion ? .opacity : .move(edge: .bottom).combined(with: .opacity))
                }
            }
            .animation(reduceMotion ? nil : .spring(duration: 0.35), value: banner?.id)
            .task(id: banner?.id) {
                guard let shown = banner else { return }
                AccessibilityNotification.Announcement(shown.announcement).post()
                guard let duration = shown.duration else { return }
                // VoiceOver users need time to reach the button.
                let seconds = voiceOver ? duration * 2 : duration
                do {
                    try await Task.sleep(for: .seconds(seconds))
                } catch {
                    // Cancelled: the banner changed, or the view went away.
                    // Neither is the time running out, so leave it be.
                    return
                }
                if banner?.id == shown.id { banner = nil }
            }
    }
}

private struct ForgeBannerView<Action>: View {
    let banner: ForgeBanner<Action>
    let tapped: () -> Void
    let dismiss: () -> Void
    @Environment(\.forgeTheme) private var theme

    var body: some View {
        HStack(spacing: theme.spacing.m) {
            if banner.showsActivity {
                ProgressView()
                    .accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: theme.spacing.xs) {
                Text(verbatim: banner.message)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(theme.colors.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
                if let progress = banner.progress {
                    ProgressView(value: progress)
                        .tint(theme.colors.accent.solid)
                        .accessibilityLabel("Progress")
                        .accessibilityValue(Text("\(Int((progress * 100).rounded())) percent"))
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if let button = banner.button {
                Button(action: tapped) {
                    Text(verbatim: button.title)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(theme.colors.accent.text)
                        .padding(.vertical, theme.spacing.xs)
                }
                .accessibilityIdentifier(button.identifier)
            }
        }
        .padding(.horizontal, theme.spacing.l)
        .padding(.vertical, theme.spacing.m)
        .frame(maxWidth: 520)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: theme.radius.l, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: theme.radius.l, style: .continuous)
                .strokeBorder(theme.colors.separator.opacity(0.6), lineWidth: 0.5)
        )
        .shadow(color: .black.opacity(0.12), radius: 12, y: 4)
        .padding(.horizontal, theme.spacing.l)
        // Room above for the shadow, which would otherwise fall on the
        // last row: the bar's measured height includes this padding.
        .padding(.top, theme.spacing.m)
        .padding(.bottom, theme.spacing.s)
        .gesture(
            DragGesture(minimumDistance: 12).onEnded { value in
                if value.translation.height > 24 { dismiss() }
            }
        )
        .accessibilityElement(children: .contain)
        .accessibilityAction(named: "Dismiss", dismiss)
        .accessibilityIdentifier("forge.banner")
    }
}
#endif
