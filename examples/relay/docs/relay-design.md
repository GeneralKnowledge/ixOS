# Relay: look and feel

Relay's visual rules. They change how Relay looks, never what it does: trust, delivery and the network stay
as the [audit](relay-audit-2026-11.md) and the other Relay docs describe them. Soundscape's
[design.md](../../soundscape/docs/design.md) follows the same glass rules.

## Liquid Glass (2026-11-24)

Glass is for controls that float over content, never for the content itself. Bubbles, the conversation
list, device rows and the history stay solid.

| Where | iOS 26 | iOS 17 to 25 |
|---|---|---|
| Chat composer (text field, attach, record, reply and edit bars) | a floating glass card over the transcript, inset from the edges | the full-width system bar |
| Assistant composer | the same floating glass card | the full-width system bar |
| A conversation's connection notice (Connecting, Reconnecting, Not connected) | a small card in the standard material under the title: it is passive, so not glass (V2.1) | the same card |
| New Messages jump button | tinted glass capsule | tinted capsule |
| Devices: Send | tinted glass in the bottom toolbar, beside plain Receive | plain toolbar button |
| Tab bar | glass, and it shrinks while a chat or list scrolls down | the opaque tab bar |

The tab bar, navigation bars, search field, menus, alerts and sheets get glass from the system; Relay adds
nothing to them.

Relay's wrappers live in `Sources/AppHost/RelayGlass.swift`; the glass itself, its iOS 17 to 25 fallbacks and
the `#if compiler(>=6.2)` / `#available(iOS 26.0, *)` checks are ForgeUI's (`forgeGlass`,
`forgeGlassProminentButton`, `forgeMaterial`, [forgeui.md](../../../docs/forgeui.md#materials-and-motion)), so
the app still builds with Xcode 16 and the deployment target stays iOS 17. Glass follows Reduce Transparency by
itself; the fallbacks and the standard material turn solid under it. Chat animations go through ForgeMotion
and stop under Reduce Motion.

iOS shows glass only to an app whose binary records the iOS 26 SDK. Linux builds get that from our xtool fork
(1.24.0-ixos.1); builds made with upstream xtool 1.24.0 record SDK 17.0 and run in compatibility mode, with
none of this.

Relay keeps the system blue accent for now.

Soundscape used the same "glass on iOS 26, material before" helper; both now call ForgeUI's (design programme
V2.1).

## Phone check

On Device Lab built with the fork xtool, iPhone on iOS 26. This proves the glass shows on a real
iOS 26 binary and that nothing became harder to read.

1. Chats: open a conversation. The composer floats as a glass card above the bottom edge, and messages
   scroll visibly under it. Attach, record, reply and edit still work, and the keyboard pushes the card up.
2. Scroll up a long chat and get a message: New Messages is a tinted glass capsule above the composer.
3. Stop the chat server (or turn Wi-Fi off): Reconnecting shows as a frosted (not glass) card under the title.
   With Reduce Motion on, New Messages and jumping to a reply move at once instead of scrolling.
4. Scroll down a chat: the tab bar shrinks; scroll back up and it returns.
5. Assistant: the composer is the same glass card.
6. Devices: Send is tinted glass in the bottom bar, Receive plain glass.
7. Settings, Accessibility, Display & Text Size, Reduce Transparency on: all of the above turn solid and
   stay readable. Repeat 1 in dark mode.
