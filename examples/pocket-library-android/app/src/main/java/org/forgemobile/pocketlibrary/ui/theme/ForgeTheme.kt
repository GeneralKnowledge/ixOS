// ForgeUI's look on Android: the shared tokens (ForgeTokens.kt) as a Material 3 colour scheme
// and typography. Android keeps its own components, layouts and motion; what carries over from
// iOS is the palette, the accent and the text roles, so the two apps read as one product.
package org.forgemobile.pocketlibrary.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight

/** The theme in effect: its colours for this appearance, spacing and radii. */
data class ForgeThemeValues(
    val colors: ForgeColors,
    val spacing: ForgeSpacing = ForgeTokens.spacing,
    val radius: ForgeRadius = ForgeTokens.radius,
)

val LocalForgeTheme = staticCompositionLocalOf { ForgeThemeValues(ForgeTokens.light) }

fun ForgeRGBA.toColor(): Color = Color(red.toFloat(), green.toFloat(), blue.toFloat(), alpha.toFloat())

/** [colors] as a Material 3 scheme, so Material components take the Forge palette. */
fun materialColors(colors: ForgeColors, dark: Boolean): ColorScheme {
    val accent = colors.accent
    val danger = colors.danger
    val base = if (dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = accent.solid.toColor(),
        onPrimary = accent.onSolid.toColor(),
        primaryContainer = accent.soft.compositedOver(colors.surface).toColor(),
        onPrimaryContainer = accent.text.toColor(),
        secondary = colors.neutral.solid.toColor(),
        onSecondary = colors.neutral.onSolid.toColor(),
        secondaryContainer = colors.neutral.soft.compositedOver(colors.surface).toColor(),
        onSecondaryContainer = colors.neutral.text.toColor(),
        tertiary = colors.info.solid.toColor(),
        onTertiary = colors.info.onSolid.toColor(),
        error = danger.solid.toColor(),
        onError = danger.onSolid.toColor(),
        errorContainer = danger.soft.compositedOver(colors.surface).toColor(),
        onErrorContainer = danger.text.toColor(),
        background = colors.background.toColor(),
        onBackground = colors.textPrimary.toColor(),
        surface = colors.background.toColor(),
        onSurface = colors.textPrimary.toColor(),
        surfaceVariant = colors.groupedBackground.toColor(),
        onSurfaceVariant = colors.textSecondary.toColor(),
        surfaceContainerLowest = colors.background.toColor(),
        surfaceContainerLow = colors.surface.toColor(),
        surfaceContainer = colors.surface.toColor(),
        surfaceContainerHigh = colors.surfaceRaised.toColor(),
        surfaceContainerHighest = colors.surfaceRaised.toColor(),
        outline = colors.textTertiary.toColor(),
        outlineVariant = colors.separator.toColor(),
    )
}

/** The Material style that plays each iOS text style's part. Sizes follow the font scale. */
fun materialStyle(style: ForgeTextStyle, type: Typography): TextStyle = when (style) {
    ForgeTextStyle.LARGE_TITLE -> type.headlineLarge
    ForgeTextStyle.TITLE -> type.headlineMedium
    ForgeTextStyle.TITLE2 -> type.headlineSmall
    ForgeTextStyle.TITLE3 -> type.titleLarge
    ForgeTextStyle.HEADLINE -> type.titleMedium
    ForgeTextStyle.SUBHEADLINE -> type.bodyMedium
    ForgeTextStyle.BODY -> type.bodyLarge
    ForgeTextStyle.CALLOUT -> type.bodyMedium
    ForgeTextStyle.FOOTNOTE -> type.bodySmall
    ForgeTextStyle.CAPTION -> type.labelMedium
    ForgeTextStyle.CAPTION2 -> type.labelSmall
}

fun ForgeTextWeight.fontWeight(): FontWeight = when (this) {
    ForgeTextWeight.REGULAR -> FontWeight.Normal
    ForgeTextWeight.MEDIUM -> FontWeight.Medium
    ForgeTextWeight.SEMIBOLD -> FontWeight.SemiBold
    ForgeTextWeight.BOLD -> FontWeight.Bold
}

/**
 * Pocket Library's theme: ForgeUI's standard palette with [accent], in light or dark. The iPhone
 * app uses `ForgeTheme(accent: .orange)`, so this does too.
 */
@Composable
fun ForgeTheme(
    accent: ForgeAccent? = null,
    dark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val base = if (dark) ForgeTokens.dark else ForgeTokens.light
    val colors = if (accent == null) base else base.withAccent(if (dark) accent.dark else accent.light)
    CompositionLocalProvider(LocalForgeTheme provides ForgeThemeValues(colors)) {
        MaterialTheme(colorScheme = materialColors(colors, dark), content = content)
    }
}

/** The theme in effect, inside [ForgeTheme]. */
object Forge {
    val colors: ForgeColors
        @Composable @ReadOnlyComposable get() = LocalForgeTheme.current.colors

    val spacing: ForgeSpacing
        @Composable @ReadOnlyComposable get() = LocalForgeTheme.current.spacing

    val radius: ForgeRadius
        @Composable @ReadOnlyComposable get() = LocalForgeTheme.current.radius

    /** The text style for [role], with its weight, colour and figures (`.forgeText(_:)` on iOS). */
    @Composable
    @ReadOnlyComposable
    fun text(role: ForgeTextRole): TextStyle {
        val colors = LocalForgeTheme.current.colors
        val colour = when (role.emphasis) {
            ForgeTextEmphasis.PRIMARY -> colors.textPrimary
            ForgeTextEmphasis.SECONDARY -> colors.textSecondary
            ForgeTextEmphasis.TERTIARY -> colors.textTertiary
        }
        return materialStyle(role.style, MaterialTheme.typography).copy(
            fontWeight = role.weight.fontWeight(),
            color = colour.toColor(),
            fontFeatureSettings = if (role.monospacedDigits) "tnum" else null,
        )
    }
}
