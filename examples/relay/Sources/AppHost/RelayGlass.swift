import ForgeUI
import SwiftUI

// Liquid Glass for the controls that float over Relay's content: the chat
// and assistant composers, the New Messages button, and Send on the Devices
// screen. Content (bubbles, lists, rows) stays solid, and a conversation's
// connection notice is passive, so it gets ForgeUI's standard material, not
// glass (docs/design/DESIGN_PHILOSOPHY.md, material by component). The glass
// itself and its iOS 17 to 25 fallbacks are ForgeUI's (`forgeGlass`); these
// wrappers keep Relay's layout around them.

extension View {
    /// A composer pinned to the bottom of a screen: a floating glass card on
    /// iOS 26, inset from the edges so the content scrolls under it; the
    /// full-width system bar background before.
    @ViewBuilder func relayFloatingBar(cornerRadius: CGFloat = 24) -> some View {
        if ForgeGlassSupport.isAvailable {
            self
                .forgeGlass(.regular, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
        } else {
            self.background(.bar)
        }
    }

    /// A small floating action over content (New Messages): tinted glass
    /// on iOS 26, the tinted capsule before.
    func relayProminentButton() -> some View {
        forgeGlassProminentButton()
    }

    /// The main action in a toolbar (Send on the Devices screen): tinted
    /// glass on iOS 26, where the toolbar is glass itself; unchanged before.
    @ViewBuilder func relayToolbarProminent() -> some View {
        if ForgeGlassSupport.isAvailable {
            self.forgeGlassProminentButton()
        } else {
            self
        }
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
