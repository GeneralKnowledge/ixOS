import SwiftUI

// Liquid Glass for the controls that float over Relay's content: the chat
// and assistant composers, a conversation's connection notice, the New
// Messages button, and Send on the Devices screen. Content (bubbles, lists,
// rows) stays solid, as Apple's guidance asks. Each piece keeps the look it
// had before on iOS 17 to 25. Glass follows Reduce Transparency by itself.
//
// The compiler checks keep these building with Xcode 16 (iOS 18 SDK), and
// the deployment target stays iOS 17 (docs/relay-design.md).

extension View {
    /// A bar pinned to the top or bottom of a screen (the composers, the
    /// connection notice): a floating glass card on iOS 26, inset from the
    /// edges so the content scrolls under it; the full-width system bar
    /// background before.
    @ViewBuilder func relayFloatingBar(cornerRadius: CGFloat = 24) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, macOS 26.0, *) {
            self
                .glassEffect(.regular, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
        } else {
            self.background(.bar)
        }
        #else
        self.background(.bar)
        #endif
    }

    /// A small floating action over content (New Messages): tinted glass
    /// on iOS 26, the tinted capsule before.
    @ViewBuilder func relayProminentButton() -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, macOS 26.0, *) {
            self.buttonStyle(.glassProminent)
        } else {
            self.buttonStyle(.borderedProminent)
        }
        #else
        self.buttonStyle(.borderedProminent)
        #endif
    }

    /// The main action in a toolbar (Send on the Devices screen): tinted
    /// glass on iOS 26, where the toolbar is glass itself; unchanged before.
    @ViewBuilder func relayToolbarProminent() -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, macOS 26.0, *) {
            self.buttonStyle(.glassProminent)
        } else {
            self
        }
        #else
        self
        #endif
    }

    /// The tab bar steps aside while a long list or a chat scrolls down,
    /// and comes back on the way up (iOS 26); nothing before.
    @ViewBuilder func relayTabBarMinimizes() -> some View {
        #if os(iOS) && compiler(>=6.2)
        if #available(iOS 26.0, *) {
            self.tabBarMinimizeBehavior(.onScrollDown)
        } else {
            self
        }
        #else
        self
        #endif
    }
}
