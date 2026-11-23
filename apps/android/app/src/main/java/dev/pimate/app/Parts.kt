package dev.pimate.app

import androidx.compose.animation.core.*
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.*

/** モック（docs/design/html）の SVG と同じ 24 の線画。色は Icon の tint で付ける。 */
private fun icon(stroke: String = "", width: Float = 2f, fill: String = ""): ImageVector =
    ImageVector.Builder(
            defaultWidth = 24.dp,
            defaultHeight = 24.dp,
            viewportWidth = 24f,
            viewportHeight = 24f,
        )
        .apply {
            if (stroke.isNotEmpty())
                addPath(
                    addPathNodes(stroke),
                    stroke = SolidColor(Color.Black),
                    strokeLineWidth = width,
                    strokeLineCap = StrokeCap.Round,
                    strokeLineJoin = StrokeJoin.Round,
                )
            if (fill.isNotEmpty()) addPath(addPathNodes(fill), fill = SolidColor(Color.Black))
        }
        .build()

private fun circle(x: Float, y: Float, r: Float) =
    "M${x - r} ${y}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"

private fun rect(x: Float, y: Float, w: Float, h: Float, r: Float) =
    "M${x + r} ${y}h${w - 2 * r}a$r $r 0 0 1 $r ${r}v${h - 2 * r}a$r $r 0 0 1 ${-r} ${r}" +
        "h${-(w - 2 * r)}a$r $r 0 0 1 ${-r} ${-r}v${-(h - 2 * r)}a$r $r 0 0 1 $r ${-r}z"

internal object Icons {
    val Menu = icon("M4 8h16M4 16h10", 1.9f)
    val Back = icon("M15 6l-6 6 6 6")
    val Down = icon("M6 9l6 6 6-6")
    val Up = icon("M6 15l6-6 6 6")
    val Close = icon("M6 6l12 12M18 6L6 18")
    val Plus = icon("M12 5v14M5 12h14", 2.2f)
    val Send = icon("M12 19V5M6 11l6-6 6 6", 2.2f)
    val Stop = icon(circle(12f, 12f, 9f), 1.8f, rect(9f, 9f, 6f, 6f, 1f))
    val Reply = icon("M9 14l-4-4 4-4M5 10h9a5 5 0 0 1 5 5v3")
    val Bulb =
        icon(
            "M9 18h6M10 21h4M12 3a6 6 0 0 0-4 10.5c.7.7 1 1.5 1 2.5h6c0-1 .3-1.8 1-2.5A6 6 0 0 0 12 3z"
        )
    val Search = icon(circle(11f, 11f, 6.5f) + "M20 20l-4.2-4.2")
    val Bell =
        icon("M12 3a6 6 0 0 0-6 6c0 5-2 7-2 7h16s-2-2-2-7a6 6 0 0 0-6-6zM10 20a2 2 0 0 0 4 0")
    val Laptop = icon(rect(4f, 5f, 16f, 11f, 1.5f) + "M2 19h20", 1.9f)
    val Server =
        icon(rect(3f, 4f, 18f, 7f, 2f) + rect(3f, 13f, 18f, 7f, 2f) + "M7 7.5h.01M7 16.5h.01", 1.9f)
    val Gear =
        icon(
            circle(12f, 12f, 3f) +
                "M12 2.5v3M12 18.5v3M4.6 4.6l2.1 2.1M17.3 17.3l2.1 2.1M2.5 12h3M18.5 12h3" +
                "M4.6 19.4l2.1-2.1M17.3 6.7l2.1-2.1",
            1.8f,
        )
    val Copy = icon(rect(9f, 9f, 11f, 11f, 2f) + "M5 15V6a2 2 0 0 1 2-2h9")
    val Check = icon("M5 12.5l4.5 4.5L19 7.5")
}

/** 作業中の印。色だけに頼らないよう、呼び出し側が隣に文字（作業中・秒数）を置く。 */
@Composable
internal fun Spinner(size: Dp = 13.dp) {
    val track = pi.surfaceStrong
    val head = pi.textSecondary
    val turn by
        rememberInfiniteTransition(label = "spinner")
            .animateFloat(0f, 360f, infiniteRepeatable(tween(900, easing = LinearEasing)), "turn")
    Canvas(Modifier.size(size).clearAndSetSemantics {}) {
        val w = this.size.minDimension * 2 / 16
        val inset = Offset(w / 2, w / 2)
        val box = Size(this.size.width - w, this.size.height - w)
        drawArc(track, 0f, 360f, false, inset, box, style = Stroke(w))
        drawArc(head, turn - 90f, 90f, false, inset, box, style = Stroke(w, cap = StrokeCap.Round))
    }
}

@Composable
internal fun Dot(color: Color, size: Dp = 6.dp, modifier: Modifier = Modifier) {
    Box(modifier.size(size).background(color, CircleShape))
}

/** 塗りの主ボタン。1画面に1つまで（Tokens）。 */
@Composable
internal fun PrimaryButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    height: Dp = 48.dp,
    icon: ImageVector? = null,
) {
    Button(
        onClick,
        modifier.heightIn(min = height),
        enabled = enabled,
        shape = CircleShape,
        colors =
            ButtonDefaults.buttonColors(
                containerColor = pi.accent,
                contentColor = pi.onAccent,
                disabledContainerColor = pi.surfaceStrong,
                disabledContentColor = pi.textSecondary,
            ),
        contentPadding = PaddingValues(start = if (icon == null) 20.dp else 14.dp, end = 20.dp),
    ) {
        if (icon != null) {
            Icon(icon, null, Modifier.size(17.dp))
            Spacer(Modifier.width(6.dp))
        }
        Text(label, style = Type.header.copy(fontSize = if (height < 48.dp) 14.sp else 15.sp))
    }
}

/** 塗りのない操作。文字の色だけで示す。 */
@Composable
internal fun TextAction(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    color: Color = pi.accentText,
    weight: FontWeight = FontWeight.SemiBold,
) {
    TextButton(
        onClick,
        modifier.heightIn(min = tap),
        enabled = enabled,
        colors =
            ButtonDefaults.textButtonColors(
                contentColor = color,
                disabledContentColor = pi.textSecondary.copy(alpha = .5f),
            ),
        contentPadding = PaddingValues(horizontal = 14.dp),
    ) {
        Text(label, style = Type.body.copy(fontWeight = weight))
    }
}

@Composable
internal fun IconAction(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    tint: Color = pi.textSecondary,
    size: Dp = 22.dp,
) {
    IconButton(onClick, modifier.size(tap), enabled = enabled) {
        Icon(icon, label, Modifier.size(size), if (enabled) tint else tint.copy(alpha = .4f))
    }
}

/** 状態の印を置く 14dp の列（ドロワーの行・ツールの行で共通）。 */
@Composable
internal fun MarkColumn(height: Dp, content: @Composable BoxScope.() -> Unit) {
    Box(Modifier.size(14.dp, height), Alignment.Center, content = content)
}
