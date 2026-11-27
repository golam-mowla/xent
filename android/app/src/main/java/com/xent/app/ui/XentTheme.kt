package com.xent.app.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

object XentColors {
    val Accent = Color(0xFF635BDB)
    val AccentDark = Color(0xFF7B74EB)
    val OnlineGreen = Color(0xFF10A37F)
    val AwayAmber = Color(0xFFF59E0B)
    val BusyRed = Color(0xFFEF4444)

    // Light palette (matches Desktop ChatGPT-minimal palette)
    val LightBg = Color(0xFFFFFFFF)
    val LightSurface = Color(0xFFF7F7F8)
    val LightSurfaceVariant = Color(0xFFEEEEF0)
    val LightText = Color(0xFF202123)
    val LightTextSecondary = Color(0xFF8E8E93)
    val LightBorder = Color(0xFFE8E8EA)

    // Dark palette
    val DarkBg = Color(0xFF212121)
    val DarkSurface = Color(0xFF171717)
    val DarkSurfaceVariant = Color(0xFF2A2A2A)
    val DarkText = Color(0xFFECECEC)
    val DarkTextSecondary = Color(0xFFA1A1AA)
    val DarkBorder = Color(0xFF3A3A3A)
}

private val LightColors = lightColorScheme(
    primary = XentColors.Accent,
    onPrimary = Color.White,
    background = XentColors.LightBg,
    onBackground = XentColors.LightText,
    surface = XentColors.LightBg,
    onSurface = XentColors.LightText,
    surfaceVariant = XentColors.LightSurface,
    onSurfaceVariant = XentColors.LightTextSecondary,
    outline = XentColors.LightBorder
)

private val DarkColors = darkColorScheme(
    primary = XentColors.AccentDark,
    onPrimary = Color.White,
    background = XentColors.DarkBg,
    onBackground = XentColors.DarkText,
    surface = XentColors.DarkSurface,
    onSurface = XentColors.DarkText,
    surfaceVariant = XentColors.DarkSurfaceVariant,
    onSurfaceVariant = XentColors.DarkTextSecondary,
    outline = XentColors.DarkBorder
)

@Composable
fun XentTheme(themePref: String, content: @Composable () -> Unit) {
    val dark = when (themePref) {
        "dark" -> true
        "light" -> false
        else -> isSystemInDarkTheme()
    }
    MaterialTheme(
        colorScheme = if (dark) DarkColors else LightColors,
        content = content
    )
}
