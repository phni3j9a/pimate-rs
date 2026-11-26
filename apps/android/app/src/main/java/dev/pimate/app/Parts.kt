package dev.pimate.app

import android.os.Build
import android.view.WindowManager
import androidx.compose.animation.core.*
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.*
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.view.WindowCompat
import kotlinx.coroutines.delay

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
    val Monitor = icon("M12 17v4m-4 0h8", fill = rect(2f, 3f, 20f, 14f, 2f))
    // TS 版と同じ単色の Grok Bot マーク（抜きの部分は even-odd）。
    val GrokBot = ImageVector.Builder(defaultWidth = 24.dp, defaultHeight = 24.dp, viewportWidth = 24f, viewportHeight = 24f).apply {
        addPath(addPathNodes("M 21.675 8.364 C 22.181 9.711 22.401 11.160 22.318 12.596 C 22.240 13.947 21.893 15.281 21.305 16.500 C 19.286 20.674 14.707 23.000 10.145 22.167 C 9.064 21.970 8.015 21.599 7.049 21.073 C 6.010 20.506 5.070 19.758 4.283 18.874 C 3.438 17.927 2.772 16.824 2.325 15.636 C 1.819 14.289 1.599 12.840 1.682 11.404 C 1.760 10.053 2.107 8.719 2.695 7.500 C 4.714 3.326 9.293 1.000 13.855 1.833 C 14.936 2.030 15.985 2.401 16.951 2.927 C 17.990 3.494 18.930 4.242 19.717 5.126 C 20.562 6.073 21.228 7.177 21.675 8.364 Z M 12.012 6.137 L 12.309 6.139 L 12.599 6.198 L 12.871 6.314 L 13.114 6.485 L 13.310 6.706 L 13.490 6.943 L 13.665 7.183 L 13.838 7.423 L 14.010 7.666 L 14.178 7.911 L 14.344 8.157 L 14.509 8.405 L 14.670 8.653 L 14.830 8.903 L 14.986 9.155 L 15.124 9.418 L 15.206 9.703 L 15.222 9.998 L 15.171 10.290 L 15.052 10.562 L 14.872 10.797 L 14.641 10.982 L 14.374 11.108 L 14.083 11.168 L 13.787 11.160 L 13.500 11.086 L 13.235 10.954 L 13.003 10.767 L 12.817 10.537 L 12.659 10.287 L 12.497 10.037 L 12.334 9.790 L 12.170 9.542 L 12.003 9.296 L 11.837 9.051 L 11.667 8.807 L 11.495 8.565 L 11.322 8.323 L 11.148 8.084 L 10.978 7.839 L 10.869 7.565 L 10.839 7.271 L 10.890 6.979 L 11.016 6.712 L 11.209 6.486 L 11.449 6.312 L 11.721 6.195 Z M 16.385 4.055 L 16.653 4.082 L 16.911 4.159 L 17.157 4.272 L 17.386 4.412 L 17.600 4.576 L 17.793 4.764 L 17.972 4.967 L 18.144 5.175 L 18.313 5.386 L 18.475 5.601 L 18.633 5.818 L 18.787 6.041 L 18.934 6.267 L 19.077 6.495 L 19.214 6.728 L 19.347 6.963 L 19.474 7.200 L 19.594 7.441 L 19.685 7.696 L 19.736 7.960 L 19.734 8.229 L 19.659 8.486 L 19.488 8.692 L 19.239 8.788 L 18.971 8.769 L 18.720 8.673 L 18.494 8.528 L 18.293 8.347 L 18.119 8.141 L 17.972 7.915 L 17.846 7.677 L 17.716 7.442 L 17.582 7.206 L 17.442 6.976 L 17.298 6.748 L 17.150 6.523 L 16.997 6.300 L 16.838 6.083 L 16.676 5.868 L 16.508 5.655 L 16.336 5.449 L 16.158 5.245 L 16.001 5.026 L 15.884 4.783 L 15.837 4.519 L 15.911 4.264 L 16.121 4.101 Z"), fill = SolidColor(Color.Black), pathFillType = PathFillType.EvenOdd)
    }.build()
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
    val Phone = icon(rect(6f, 2f, 12f, 20f, 2f) + "M11 19h2")
    val Laptop = icon(rect(4f, 5f, 16f, 11f, 1.5f) + "M2 19h20", 1.9f)
    val Server =
        icon(rect(3f, 4f, 18f, 7f, 2f) + rect(3f, 13f, 18f, 7f, 2f) + "M7 7.5h.01M7 16.5h.01", 1.9f)
    // 設定の歯車は TS 版と同じ Lucide 1.72.0 の settings（ISC、assets/licenses）。
    val Gear =
        icon(
            circle(12f, 12f, 3f) +
                "M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915" +
                " 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033" +
                " 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915" +
                " 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051" +
                "a2.34 2.34 0 0 0 3.319-1.915",
            1.8f,
        )
    val Copy = icon(rect(9f, 9f, 11f, 11f, 2f) + "M5 15V6a2 2 0 0 1 2-2h9")
    val Check = icon("M5 12.5l4.5 4.5L19 7.5")
    val Right = icon("M9 6l6 6-6 6")
    // アプリの外（ブラウザ）で開く印。
    val External =
        icon("M15 3h6v6M10 14L21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6")
    val More = icon(fill = circle(5f, 12f, 1.6f) + circle(12f, 12f, 1.6f) + circle(19f, 12f, 1.6f))
    val Clock = icon(circle(12f, 12f, 9f) + "M12 7v5l3 2")
    val Swap = icon("M4 7h14l-3-3M20 17H6l3 3")
    val Terminal = icon(rect(3f, 4f, 18f, 13f, 2f) + "M8 21h8M7 9l3 2-3 2M12 13h4", 1.9f)
    val Context =
        icon(
            "M7 3h10a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" +
                "M9.5 8h5M9.5 12h5M9.5 16h3",
            2.4f,
        )
    private const val STAR = "M12 3.2l2.7 5.6 6.1.8-4.5 4.3 1.1 6.1L12 17.1 6.6 20l1.1-6.1-4.5-4.3 6.1-.8z"
    val Star = icon(STAR, 1.8f)
    val StarOn = icon(STAR, 1.8f, STAR)
    // 各社のマーク（simple-icons 16.35.0 の図形を改変せずに使う）。利用枠がどのプロバイダーのものかを示すためだけに置く。
    val Claude =
        icon(
            fill =
                "m4.7144 15.9555 " +
            "4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 " +
            "1.5179.1032 2.2767.1578 1.6514.0972 " +
            "2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 " +
            "9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 " +
            "1.9064 1.4754 2.4893 " +
            "1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 " +
            "6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 " +
            "3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 " +
            "1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 " +
            "1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 " +
            "1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 " +
            "1.5421-.2794 " +
            "1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 " +
            "1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 " +
            "1.0686 2.0035 1.8092 2.5075 " +
            "2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 " +
            "2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 " +
            "17.959l-1.1414-1.9428-.1397.079-.674 " +
            "7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 " +
            "1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 " +
            "2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 " +
            "4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"
        )
    // GitHub のマーク（単色のみ）。設定の GitHub へのリンクにだけ置く。
    val GitHub =
        icon(
            fill =
                "M12 .297" +
                "c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61" +
                "C4.422 18.07 3.633 17.7 3.633 17.7" +
                "c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57" +
                "C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"
        )
    val OpenAI =
        icon(
            fill =
                "M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 " +
            "0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 " +
            "6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 " +
            "2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 " +
            "5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 " +
            "4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 " +
            ".3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 " +
            "1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 " +
            "2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 " +
            "1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 " +
            "4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 " +
            "1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 " +
            "7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 " +
            "2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 " +
            "0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 " +
            "9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 " +
            "4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 " +
            "0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 " +
            "0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 " +
            "1.4997-2.6067-1.4997Z"
        )
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
    style: TextStyle = Type.body,
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
        Text(label, style = style.copy(fontWeight = weight))
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

/** 下から出すシート（角丸 16、背景と同じ色、つまみ）。中身は必要なぶんだけの高さで、長ければ中で送る。 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun PiSheet(close: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    val density = LocalDensity.current
    ModalBottomSheet(
        close,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        shape = RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp),
        containerColor = pi.background,
        tonalElevation = 0.dp,
        scrimColor = pi.scrim,
        dragHandle = {
            Box(
                Modifier.padding(top = 8.dp, bottom = 14.dp)
                    .size(36.dp, 4.dp)
                    .background(pi.surfaceStrong, CircleShape)
            )
        },
    ) {
        // シートも別の窓なので、設定の文字サイズを引き継ぐ。
        CompositionLocalProvider(LocalDensity provides density) {
            Column(
                Modifier.verticalScroll(rememberScrollState())
                    .padding(start = gutter, end = gutter, bottom = 24.dp),
                content = content,
            )
        }
    }
}

/** シートの見出しと、右の補足。 */
@Composable
internal fun SheetTitle(title: String, note: String = "", style: TextStyle = Type.sheet) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(title, Modifier.weight(1f).semantics { heading() }, style = style)
        if (note.isNotEmpty())
            Text(note, Modifier.padding(start = 12.dp), pi.textSecondary, style = Type.caption)
    }
}

/** セグメント: 濃い塗りの台に、選択中だけ背景色の面（Tokens）。 */
@Composable
internal fun Segment(
    options: List<Pair<String, String>>,
    selected: String?,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    // 設定の行の右に置く小さい版（幅は呼び出し側が決める）。
    compact: Boolean = false,
    select: (String) -> Unit,
) {
    Row(
        (if (compact) modifier else modifier.fillMaxWidth())
            .background(pi.surfaceStrong, RoundedCornerShape(10.dp))
            .padding(3.dp)
            .selectableGroup()
    ) {
        options.forEach { (value, label) ->
            val on = value == selected
            Box(
                Modifier.weight(1f)
                    .clip(RoundedCornerShape(if (compact) 7.dp else 8.dp))
                    .background(if (on) pi.background else pi.surfaceStrong)
                    .selectable(on, enabled, Role.RadioButton) { if (!on) select(value) }
                    .heightIn(min = if (compact) 30.dp else 42.dp)
                    .padding(horizontal = 2.dp),
                Alignment.Center,
            ) {
                Text(
                    label,
                    color = if (on) pi.text else pi.textSecondary,
                    style =
                        Type.body.copy(
                            fontSize = if (compact) 12.sp else 13.sp,
                            fontWeight = if (on) FontWeight.SemiBold else FontWeight.Normal,
                        ),
                    maxLines = 1,
                )
            }
        }
    }
}

/** PC のターミナルに貼るコマンド（カプセル型）。右のボタンでコピーする。surface: 面の上に置くとき（背景色で抜く）。 */
@Composable
internal fun CommandPill(command: String, modifier: Modifier = Modifier, onSurface: Boolean = false) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) {
        if (copied) {
            delay(1500)
            copied = false
        }
    }
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = tap)
            .background(if (onSurface) pi.background else pi.surface, CircleShape)
            .padding(start = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            command,
            Modifier.weight(1f),
            style = Type.mono.copy(fontSize = 13.sp),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        IconAction(
            if (copied) Icons.Check else Icons.Copy,
            if (copied) "コピーしました" else "コマンドをコピー",
            {
                clipboard.setText(AnnotatedString(command))
                copied = true
            },
            size = 18.dp,
        )
    }
}

/** 使用量の棒（4dp）。不明は空のまま。 */
@Composable
internal fun UsageBar(percent: Double?, label: String, modifier: Modifier = Modifier) {
    val fill =
        when (usage(percent)) {
            Usage.Full -> pi.failed
            Usage.High -> pi.waiting
            Usage.Normal -> pi.text
        }
    Box(
        modifier
            .fillMaxWidth()
            .height(4.dp)
            .background(pi.surfaceStrong, CircleShape)
            .semantics(mergeDescendants = true) {
                contentDescription = label
                if (percent == null) stateDescription = "不明"
                else progressBarRangeInfo = ProgressBarRangeInfo(percent.toFloat(), 0f..100f)
            }
    ) {
        if (percent != null && percent > 0)
            Box(
                Modifier.fillMaxWidth((percent / 100).toFloat().coerceIn(0f, 1f))
                    .height(4.dp)
                    .background(fill, CircleShape)
            )
    }
}

/** 使用状況のリング（24dp）。不明は点線にして、0% と読めないようにする。 */
@Composable
internal fun UsageRing(percent: Double?, content: @Composable () -> Unit) {
    val track = pi.surfaceStrong
    val unknown = pi.textTertiary
    val fill =
        when (usage(percent)) {
            Usage.Full -> pi.failed
            Usage.High -> pi.waiting
            Usage.Normal -> pi.textSecondary
        }
    Box(Modifier.size(24.dp), Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val w = 2.dp.toPx()
            val inset = Offset(w / 2, w / 2)
            val box = Size(size.width - w, size.height - w)
            if (percent == null)
                drawArc(
                    unknown,
                    0f,
                    360f,
                    false,
                    inset,
                    box,
                    style =
                        Stroke(
                            w,
                            pathEffect =
                                PathEffect.dashPathEffect(floatArrayOf(1.5.dp.toPx(), 3.dp.toPx())),
                        ),
                )
            else {
                drawArc(track, 0f, 360f, false, inset, box, style = Stroke(w))
                if (percent > 0)
                    drawArc(
                        fill,
                        -90f,
                        (percent.coerceIn(0.0, 100.0) * 3.6).toFloat(),
                        false,
                        inset,
                        box,
                        style = Stroke(w, cap = StrokeCap.Round),
                    )
            }
        }
        content()
    }
}

/**
 * 画面全体を使うダイアログ（表・画像・ツリー）。ステータスバーの下まで描き、帯の文字の色を中身の背景に合わせる。 dark: 中身の背景が暗い。
 */
@Composable
internal fun FullScreen(
    close: () -> Unit,
    dark: Boolean = darkPalette,
    content: @Composable () -> Unit,
) {
    // 別の窓の中では密度が端末の値に戻る。設定の文字サイズを引き継ぐ。
    val density = LocalDensity.current
    Dialog(
        close,
        DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        val view = LocalView.current
        val window = (view.parent as? DialogWindowProvider)?.window
        SideEffect {
            window?.run {
                setDimAmount(0f)
                // ダイアログの窓は既定でシステムの帯を避けて置かれる。帯の下まで広げる。
                setLayout(
                    WindowManager.LayoutParams.MATCH_PARENT,
                    WindowManager.LayoutParams.MATCH_PARENT,
                )
                if (Build.VERSION.SDK_INT >= 30)
                    attributes =
                        attributes.apply {
                            fitInsetsTypes = 0
                            // 既定のままだと、画面上部の切り欠きの高さだけ窓が下がる。
                            layoutInDisplayCutoutMode =
                                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
                        }
                else addFlags(WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS)
                WindowCompat.getInsetsController(this, view).run {
                    isAppearanceLightStatusBars = !dark
                    isAppearanceLightNavigationBars = !dark
                }
            }
        }
        CompositionLocalProvider(LocalDensity provides density, content = content)
    }
}
