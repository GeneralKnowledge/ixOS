// The types behind ForgeUI's tokens on Android, the counterparts of
// forgekit/Sources/ForgeUI/Tokens. Plain Kotlin, so JVM tests check a theme's contrast. The
// values themselves are generated into ForgeTokens.kt from the Swift tokens.
package org.forgemobile.pocketlibrary.ui.theme

import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/** One colour in sRGB, components 0..1. */
data class ForgeRGBA(val red: Double, val green: Double, val blue: Double, val alpha: Double = 1.0) {
    /** `0xRRGGBB`, for example `ForgeRGBA(0x0062CC)`. */
    constructor(hex: Int, alpha: Double = 1.0) : this(
        ((hex shr 16) and 0xFF) / 255.0,
        ((hex shr 8) and 0xFF) / 255.0,
        (hex and 0xFF) / 255.0,
        alpha,
    )

    fun opacity(alpha: Double) = copy(alpha = alpha)

    /** This colour drawn over an opaque [background]: what the eye sees. */
    fun compositedOver(background: ForgeRGBA) = ForgeRGBA(
        red * alpha + background.red * (1 - alpha),
        green * alpha + background.green * (1 - alpha),
        blue * alpha + background.blue * (1 - alpha),
    )

    /** WCAG relative luminance of the opaque colour. */
    val luminance: Double
        get() {
            fun channel(c: Double) = if (c <= 0.03928) c / 12.92 else ((c + 0.055) / 1.055).pow(2.4)
            return 0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
        }

    /** WCAG contrast ratio with [other] (both opaque), 1 to 21. */
    fun contrast(other: ForgeRGBA): Double {
        val a = luminance
        val b = other.luminance
        return (max(a, b) + 0.05) / (min(a, b) + 0.05)
    }
}

/**
 * The colours of one tone. `solid` fills a shape and `onSolid` is text on it; `soft` is a
 * tinted background and `text` is the tone's readable colour on `soft` or on any surface.
 */
data class ForgeToneColors(
    val solid: ForgeRGBA,
    val onSolid: ForgeRGBA,
    val text: ForgeRGBA,
    val softOpacity: Double,
) {
    val soft: ForgeRGBA get() = solid.opacity(softOpacity)
}

enum class ForgeTone { ACCENT, NEUTRAL, SUCCESS, WARNING, DANGER, INFO }

/** Every colour role in one appearance (light or dark). */
data class ForgeColors(
    val background: ForgeRGBA,
    val groupedBackground: ForgeRGBA,
    val surface: ForgeRGBA,
    val surfaceRaised: ForgeRGBA,
    val textPrimary: ForgeRGBA,
    val textSecondary: ForgeRGBA,
    val textTertiary: ForgeRGBA,
    val separator: ForgeRGBA,
    val accent: ForgeToneColors,
    val neutral: ForgeToneColors,
    val success: ForgeToneColors,
    val warning: ForgeToneColors,
    val danger: ForgeToneColors,
    val info: ForgeToneColors,
) {
    fun tone(tone: ForgeTone): ForgeToneColors = when (tone) {
        ForgeTone.ACCENT -> accent
        ForgeTone.NEUTRAL -> neutral
        ForgeTone.SUCCESS -> success
        ForgeTone.WARNING -> warning
        ForgeTone.DANGER -> danger
        ForgeTone.INFO -> info
    }

    /** Every opaque background text may sit on. */
    val backgrounds: List<Pair<String, ForgeRGBA>>
        get() = listOf(
            "background" to background,
            "groupedBackground" to groupedBackground,
            "surface" to surface,
            "surfaceRaised" to surfaceRaised,
        )

    fun withAccent(accent: ForgeToneColors) = copy(accent = accent)
}

data class ForgeSpacing(val xxs: Float, val xs: Float, val s: Float, val m: Float, val l: Float, val xl: Float, val xxl: Float)

data class ForgeRadius(val s: Float, val m: Float, val l: Float)

/** iOS's text styles; ForgeTheme maps each to the Material 3 style that plays its part. */
enum class ForgeTextStyle { LARGE_TITLE, TITLE, TITLE2, TITLE3, HEADLINE, SUBHEADLINE, BODY, CALLOUT, FOOTNOTE, CAPTION, CAPTION2 }

enum class ForgeTextWeight { REGULAR, MEDIUM, SEMIBOLD, BOLD }

enum class ForgeTextEmphasis { PRIMARY, SECONDARY, TERTIARY }

/**
 * A theme's contrast problems, by the same rules as ForgeUI's `contrastIssues()` on iOS: text
 * at WCAG AA (4.5:1), solid shapes at 3:1. Empty when fine.
 */
fun ForgeColors.contrastIssues(): List<String> {
    val issues = mutableListOf<String>()
    fun check(what: String, fg: ForgeRGBA, bg: ForgeRGBA, minimum: Double) {
        val ratio = fg.compositedOver(bg).contrast(bg)
        if (ratio < minimum) issues += "$what: %.2f:1 (needs $minimum)".format(ratio)
    }
    for ((name, bg) in backgrounds) {
        check("textPrimary on $name", textPrimary, bg, 4.5)
        check("textSecondary on $name", textSecondary, bg, 4.5)
        check("textTertiary on $name", textTertiary, bg, 4.5)
        for (tone in ForgeTone.entries) {
            val t = tone(tone)
            val label = tone.name.lowercase()
            check("$label text on $name", t.text, bg, 4.5)
            check("$label text on $label soft over $name", t.text, t.soft.compositedOver(bg), 4.5)
            check("$label solid against $name", t.solid, bg, 3.0)
        }
    }
    for (tone in ForgeTone.entries) {
        val t = tone(tone)
        check("${tone.name.lowercase()} onSolid on solid", t.onSolid, t.solid, 4.5)
    }
    return issues
}
