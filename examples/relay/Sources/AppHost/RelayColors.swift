import ForgeUI

// Relay's colours (docs/design/DESIGN_TOKENS.md, Relay tokens): calm and
// precise, a muted blue accent instead of the system blue. Plain ForgeUI
// tokens, Foundation only, so AppHostTests check their contrast the way
// ForgeUITests checks ForgeUI's own palette.
//
// Adopted now (V2.2): the accent, through `theme`, for outgoing bubbles,
// links, selection and the tab bar. The backgrounds and text colours below
// are the redesign's (V4); the screens keep the system's list backgrounds
// and label colours until they move off system lists, so a ForgeUI card is
// never midnight on a black list.
//
// Delivery and trust colours stay semantic (Not Delivered is ForgeUI's
// danger tone, trust badges keep their own colours), never the accent.
enum RelayColors {
    /// Dark-mode background.
    static let midnight = ForgeRGBA(hex: 0x111317)
    /// Dark-mode surface: incoming bubbles, device cards.
    static let surfaceDark = ForgeRGBA(hex: 0x20242B)
    /// Dark-mode primary text.
    static let textDark = ForgeRGBA(hex: 0xE5E7EB)
    /// Timestamps and previews. The light one is a step darker than the
    /// brief's #6B7280, which is 4.3:1 on the grouped background.
    static let secondaryDark = ForgeRGBA(hex: 0x9CA3AF)
    static let secondaryLight = ForgeRGBA(hex: 0x646B78)

    /// The brief's accent, #829BCB, is 2.8:1 on white and with white text,
    /// so light mode has a darker partner and dark mode puts midnight text
    /// on it.
    static let accent = ForgeToneColors(
        solid: ForgeRGBA(hex: 0x3E5A8E), onSolid: ForgeRGBA(hex: 0xFFFFFF),
        text: ForgeRGBA(hex: 0x3E5A8E), softOpacity: ForgePalette.lightSoft)
    static let accentDark = ForgeToneColors(
        solid: ForgeRGBA(hex: 0x829BCB), onSolid: midnight,
        text: ForgeRGBA(hex: 0xA9BBDD), softOpacity: ForgePalette.darkSoft)

    /// ForgeUI's standard palette with Relay's accent. `forgeTheme` also
    /// tints plain controls with it.
    static let theme = ForgeTheme(palette: ForgePalette.standard.withAccent(light: accent, dark: accentDark))
}
