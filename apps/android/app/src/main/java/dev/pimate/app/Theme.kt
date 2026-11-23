package dev.pimate.app

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.*
import androidx.compose.ui.text.intl.LocaleList
import androidx.compose.ui.text.style.LineBreak
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.*

// docs/design/html/Tokens.html. 画面の左右 20 / ターンの間 32 / タップ領域は Android の 48。
val gutter = 20.dp
internal val turnGap = 32.dp
internal val tap = 48.dp

/** 色の役割は Tokens のまま。ダークは同じ役割を暗い色に置き換える（docs/SPEC.md「UIの方向性」）。 */
@Immutable
internal data class Palette(
    val background: Color,
    val surface: Color,
    val surfaceStrong: Color,
    val selected: Color,
    val text: Color,
    val textSecondary: Color,
    val textCode: Color,
    val textTertiary: Color,
    val accent: Color,
    val accentText: Color,
    val onAccent: Color,
    val waiting: Color,
    val waitingText: Color,
    val failed: Color,
    val failedText: Color,
    val done: Color,
    val addText: Color,
    val delText: Color,
    val scrim: Color,
    // PC の Pi の絵の中だけで使う（UI の配色には使わない）。
    val piBlue: Color,
    val piCoral: Color,
    val piYellow: Color,
    val piRule: Color,
)

private val Light =
    Palette(
        background = Color(0xFFFFFFFF),
        surface = Color(0xFFF1F3F4),
        surfaceStrong = Color(0xFFE6E9EB),
        selected = Color(0xFFDFE7EC),
        text = Color(0xFF1F2326),
        textSecondary = Color(0xFF5E686D),
        textCode = Color(0xFF3A4246),
        textTertiary = Color(0xFF8A9499),
        accent = Color(0xFF1B6B91),
        accentText = Color(0xFF155A7A),
        onAccent = Color(0xFFFFFFFF),
        waiting = Color(0xFFE1AD42),
        waitingText = Color(0xFF8B5D20),
        failed = Color(0xFFE37769),
        failedText = Color(0xFFA84D45),
        done = Color(0xFF4D9ABF),
        addText = Color(0xFF22607E),
        delText = Color(0xFF9A453E),
        scrim = Color(0x4D10181C),
        piBlue = Color(0xFF4C93BC),
        piCoral = Color(0xFFF38C7B),
        piYellow = Color(0xFFF5C15E),
        piRule = Color(0x731B6B91),
    )

private val Dark =
    Palette(
        background = Color(0xFF131618),
        surface = Color(0xFF1E2326),
        surfaceStrong = Color(0xFF2A3034),
        selected = Color(0xFF233844),
        text = Color(0xFFE4E8EA),
        textSecondary = Color(0xFF9AA5AA),
        textCode = Color(0xFFC3CBCF),
        textTertiary = Color(0xFF6D777C),
        accent = Color(0xFF2C7DA5),
        accentText = Color(0xFF7CBEDE),
        onAccent = Color(0xFFFFFFFF),
        waiting = Color(0xFFE1AD42),
        waitingText = Color(0xFFE9C47E),
        failed = Color(0xFFE37769),
        failedText = Color(0xFFF09C91),
        done = Color(0xFF4D9ABF),
        addText = Color(0xFF9DCCE4),
        delText = Color(0xFFF09C91),
        scrim = Color(0x8C000000),
        piBlue = Color(0xFF329DFD),
        piCoral = Color(0xFFFC7466),
        piYellow = Color(0xFFFDCA3D),
        piRule = Color(0x8C7CBEDE),
    )

internal val LocalPalette = staticCompositionLocalOf { Light }
internal val darkPalette: Boolean
    @Composable get() = LocalPalette.current === Dark

internal val pi: Palette
    @Composable @ReadOnlyComposable get() = LocalPalette.current

internal val Mono =
    FontFamily(
        Font(R.font.jetbrains_mono_regular, FontWeight.Normal),
        Font(R.font.jetbrains_mono_semibold, FontWeight.SemiBold),
    )

/**
 * 文字は Tokens の5段（17・15・14・12・11）と Mono 12。行の高さは本文の 1.5 倍。
 * 言語は日本語に固定する（端末が他言語でも漢字を日本の字形で出し、文節で折り返せるように）。
 */
internal object Type {
    // 行の高さは CSS と同じく各行の上下に均等に配る（和文と欧文・等幅が混ざっても、表の行で文字の高さがそろう）。
    private val base =
        TextStyle(
            localeList = LocaleList("ja"),
            lineHeightStyle =
                LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.None),
        )
    val title = base.copy(fontSize = 22.sp, lineHeight = 30.sp, fontWeight = FontWeight.SemiBold)
    val sheet = base.copy(fontSize = 17.sp, lineHeight = 25.5.sp, fontWeight = FontWeight.SemiBold)
    val header = base.copy(fontSize = 15.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold)
    val lead = base.copy(fontSize = 15.sp, lineHeight = 22.sp)
    val body = base.copy(fontSize = 14.sp, lineHeight = 21.sp)
    val caption = base.copy(fontSize = 12.sp, lineHeight = 18.sp)
    val meta = base.copy(fontSize = 11.sp, lineHeight = 16.5.sp)
    val mono = base.copy(fontSize = 12.sp, lineHeight = 18.sp, fontFamily = Mono)
    // 短い文は文節で折り返す（モックの word-break: auto-phrase。Android 13 以降で有効）。
    val phrase = LineBreak.Heading
}

/** 設定の文字サイズ: 小 0.875・標準・大 1.15（docs/SPEC.md）。OS の倍率に掛ける。 */
internal val fontScales = listOf("小" to .875f, "標準" to 1f, "大" to 1.15f)

@Composable
fun PiTheme(scale: Float, mode: ThemeMode = ThemeMode.SYSTEM, content: @Composable () -> Unit) {
    val density = LocalDensity.current
    val dark = mode.dark(isSystemInDarkTheme())
    val p = if (dark) Dark else Light
    // Material の部品（シート・メニュー・選択ハンドル）も同じ役割の色を引く。面の色付けと影は使わない。
    val base = if (dark) darkColorScheme() else lightColorScheme()
    val colors =
        base.copy(
            primary = p.accent,
            onPrimary = p.onAccent,
            background = p.background,
            onBackground = p.text,
            surface = p.background,
            onSurface = p.text,
            onSurfaceVariant = p.textSecondary,
            surfaceVariant = p.surface,
            surfaceContainerLowest = p.background,
            surfaceContainerLow = p.background,
            surfaceContainer = p.surface,
            surfaceContainerHigh = p.surface,
            surfaceContainerHighest = p.surfaceStrong,
            secondaryContainer = p.selected,
            onSecondaryContainer = p.text,
            surfaceTint = Color.Transparent,
            outline = p.textTertiary,
            outlineVariant = p.surfaceStrong,
            error = p.failedText,
            scrim = p.scrim,
        )
    CompositionLocalProvider(
        LocalDensity provides Density(density.density, density.fontScale * scale),
        LocalPalette provides p,
        LocalTextSelectionColors provides TextSelectionColors(p.accent, p.accent.copy(alpha = .3f)),
    ) {
        MaterialTheme(
            colorScheme = colors,
            typography =
                Typography(
                    bodyLarge = Type.body,
                    bodyMedium = Type.body,
                    titleMedium = Type.header,
                    labelLarge = Type.body.copy(fontWeight = FontWeight.SemiBold),
                    labelSmall = Type.meta,
                ),
        ) {
            CompositionLocalProvider(
                LocalContentColor provides p.text,
                LocalTextStyle provides Type.body,
                content = content,
            )
        }
    }
}
