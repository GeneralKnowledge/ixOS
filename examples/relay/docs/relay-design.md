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
| A conversation's connection notice (Connecting, Reconnecting, Not connected) | a small card in the standard material under the title: it is passive, so not glass (V2.1); no tinted fill inside it (`relayPassiveNotice`) | the same card |
| Chats list notices (can't reach the chat server, messages waiting, account refused) | the same standard-material card as a row of its own, at the search field's width | the same card |
| Chats and Assistant lists | full width (plain list) like Messages, unread dot hanging in the margin so names line up with the search field | the same |
| New Messages jump button | tinted glass capsule | tinted capsule |
| Devices: Send and Receive | floating over the list just above the tab bar: Send tinted glass, Receive a glass capsule (not a bottom toolbar, which iOS 26 puts behind the floating tab bar) | Send bordered prominent, Receive a material capsule |
| Tab bar | glass, and it shrinks while a chat or list scrolls down | the opaque tab bar |

The tab bar, navigation bars, search field, menus, alerts and sheets get glass from the system; Relay adds
nothing to them.

Relay's wrappers live in `Sources/AppHost/RelayGlass.swift`; the glass itself, its iOS 17 to 25 fallbacks and
the `#if compiler(>=6.2)` / `#available(iOS 26.0, *)` checks are ForgeUI's (`forgeGlass`,
`forgeGlassProminentButton`, `forgeMaterial`, [forgeui.md](../../../docs/forgeui.md#materials-and-motion)), so
the app still builds with Xcode 16 and the deployment target stays iOS 17. Glass follows Reduce Transparency by
itself; the fallbacks and the standard material turn solid under it. All seven chat animations (scroll to the
newest message, New Messages appearing and going, jumping to a quoted reply and its highlight fading) are
ForgeMotion `standard` and stop under Reduce Motion; Relay has no other animations of its own. ForgeUI's
button press (0.12 s) and the Undo banner (spring 0.35 s) already follow Reduce Motion.

iOS shows glass only to an app whose binary records the iOS 26 SDK. Linux builds get that from our xtool fork
(1.24.0-ixos.1); builds made with upstream xtool 1.24.0 record SDK 17.0 and run in compatibility mode, with
none of this.

## Colour (2026-11-24, design programme V2.2)

Relay's accent is a muted blue instead of the system blue (`Sources/AppHost/RelayColors.swift`, values in
`docs/design/DESIGN_TOKENS.md`, Relay tokens), applied with `forgeTheme` at the root so Device Lab's sheet gets
it too. It is adopted subject to review on the phone.

| Where | Light | Dark |
|---|---|---|
| Our bubbles, the assistant's question bubbles | #3E5A8E with white text | #829BCB with midnight (#111317) text |
| Links, selection, tab bar, unread dot, New Messages, quote and reply bars, voice waveform | #3E5A8E | #829BCB (tinted text #A9BBDD) |
| A message that failed to send | ForgeUI's danger tone with its own text colour | the same |
| Trust badges, delivery captions, received transfers | unchanged, semantic, never the accent | the same |

The brief's #829BCB is 2.8:1 on white and with white text on it, hence the darker light partner and dark text in
dark mode. A sending bubble now keeps the full accent (at 60% its text fell to 2.8:1); the caption under it
already says Sending. Text and quote bars on our bubbles take the bubble's text colour, so they stay readable
on the accent and on a failed bubble alike. `RelayColorsTests` checks every pairing ForgeUI draws
(`contrastIssues()`, light, dark and both with Increase Contrast) and the redesign's own tokens.

Backgrounds and text stay the system's (lists, labels) until the screens are redesigned (V4); the midnight
background and its surfaces are defined and tested but not used yet. An "Offline" line on Chats is also V4.1:
the Chats list already says when messages are waiting or the server can't be reached.

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
6. Devices: Send (tinted glass) and Receive (glass capsule) sit above the tab bar, fully visible and tappable, including after scrolling the list down and back up.
7. Settings, Accessibility, Display & Text Size, Reduce Transparency on: all of the above turn solid and
   stay readable. Repeat 1 in dark mode.
8. Colour: in light and dark mode, our bubbles are the muted blue (white text in light, dark text in dark), a
   failed message is red, the selected tab and links are the same blue, and nothing is the old system blue.
   Send a message with the server stopped: the sending bubble's text stays readable.
