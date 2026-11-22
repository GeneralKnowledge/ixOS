package org.forgemobile.relay.ui.theme

import org.junit.Assert.assertEquals
import org.junit.Test

/** The Android tokens keep ForgeUI's guarantees: every pairing readable, every accent too. */
class ForgeTokensTest {
    @Test
    fun theStandardPaletteMeetsContrastInLightAndDark() {
        assertEquals(emptyList<String>(), ForgeTokens.light.contrastIssues())
        assertEquals(emptyList<String>(), ForgeTokens.dark.contrastIssues())
    }

    @Test
    fun everyAccentMeetsContrastInLightAndDark() {
        for (accent in ForgeAccent.entries) {
            assertEquals("$accent light", emptyList<String>(), ForgeTokens.light.withAccent(accent.light).contrastIssues())
            assertEquals("$accent dark", emptyList<String>(), ForgeTokens.dark.withAccent(accent.dark).contrastIssues())
        }
    }

    @Test
    fun valuesMatchTheSwiftTokens() {
        // Spot checks against forgekit/Sources/ForgeUI/Tokens (the generator copies them all).
        assertEquals(ForgeRGBA(0xB04A00), ForgeAccent.ORANGE.light.solid)
        assertEquals(ForgeRGBA(0xF2F2F7), ForgeTokens.light.groupedBackground)
        assertEquals(0.24, ForgeAccent.BLUE.dark.softOpacity, 0.0)
        assertEquals(ForgeTextStyle.HEADLINE, ForgeTextRole.ROW_TITLE.style)
        assertEquals(16f, ForgeTokens.spacing.l)
    }

    @Test
    fun contrastIsWcag() {
        assertEquals(21.0, ForgeRGBA(0x000000).contrast(ForgeRGBA(0xFFFFFF)), 0.01)
        assertEquals(1.0, ForgeRGBA(0x777777).contrast(ForgeRGBA(0x777777)), 0.0001)
    }
}
