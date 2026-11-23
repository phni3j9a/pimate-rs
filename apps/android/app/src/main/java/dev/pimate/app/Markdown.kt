package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.DisableSelection
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.*
import androidx.compose.ui.text.font.*
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.*
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import kotlinx.coroutines.*
import uniffi.pimate_ffi.*

private fun blocks(source: String) = parse(source, false).use { it.blocks() }

/** 回答の本文。長押しで OS の選択になり、範囲を広げてコピーできる（選択は 1 つの回答の中）。 live（逐次表示）は増えるたびに別スレッドで解析し、前の表示を残したまま差し替える。 */
@Composable
internal fun Markdown(source: String, modifier: Modifier = Modifier, live: Boolean = false) {
    val parsed =
        if (live)
            produceState(emptyList<Block>(), source) {
                    value = withContext(Dispatchers.Default) { blocks(source) }
                }
                .value
        else remember(source) { blocks(source) }
    SelectionContainer(modifier) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            parsed.forEach { key(it.id) { BlockView(it) } }
        }
    }
}

private const val CODE = "code"

private fun annotate(spans: List<Inline>, p: Palette) = buildAnnotatedString {
    spans.forEach { span ->
        when (span) {
            is Inline.Image -> append(span.v1.alt.ifEmpty { "画像" })
            is Inline.Text -> {
                val s = span.style
                // href は中核が http(s) だけに絞っている。
                val url = s.href?.value()
                if (url != null)
                    pushLink(
                        LinkAnnotation.Url(
                            url,
                            TextLinkStyles(
                                SpanStyle(
                                    color = p.accentText,
                                    textDecoration = TextDecoration.Underline,
                                )
                            ),
                        )
                    )
                if (s.code) pushStringAnnotation(CODE, "")
                withStyle(
                    SpanStyle(
                        fontWeight = if (s.bold) FontWeight.SemiBold else null,
                        fontStyle = if (s.italic) FontStyle.Italic else null,
                        fontFamily = if (s.code) Mono else null,
                        fontSize = if (s.code) .9.em else TextUnit.Unspecified,
                        textDecoration = if (s.strike) TextDecoration.LineThrough else null,
                    )
                ) {
                    append(span.text)
                }
                if (s.code) pop()
                if (url != null) pop()
            }
        }
    }
}

/** インラインコードの面。文字は足さない（部分コピーに余分な空白を混ぜない）。 */
private fun DrawScope.chips(l: TextLayoutResult, text: AnnotatedString, color: Color) {
    if (l.layoutInput.text.length != text.length) return
    val out = 2.dp.toPx()
    val radius = CornerRadius(5.dp.toPx())
    for (a in text.getStringAnnotations(CODE, 0, text.length)) {
        if (a.start >= a.end) continue
        for (line in l.getLineForOffset(a.start)..l.getLineForOffset(a.end - 1)) {
            val from = maxOf(a.start, l.getLineStart(line))
            val to = minOf(a.end, l.getLineEnd(line, visibleEnd = true))
            if (from >= to) continue
            val left = l.getBoundingBox(from).left
            val right = l.getBoundingBox(to - 1).right
            val top = l.getLineTop(line)
            val height = l.getLineBottom(line) - top
            val chip = minOf(height, height * .88f)
            drawRoundRect(
                color,
                Offset(left - out, top + (height - chip) / 2),
                Size(right - left + 2 * out, chip),
                radius,
            )
        }
    }
}

@Composable
private fun Spans(
    spans: List<Inline>,
    style: TextStyle = LocalTextStyle.current,
    modifier: Modifier = Modifier,
    align: TextAlign? = null,
) {
    val p = pi
    val text = remember(spans, p) { annotate(spans, p) }
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    Text(
        text,
        modifier.drawBehind { layout?.let { chips(it, text, p.surface) } },
        style = style,
        textAlign = align,
        onTextLayout = { layout = it },
    )
}

@Composable
private fun BlockView(block: Block) {
    when (val c = block.content) {
        is Content.Paragraph -> Spans(c.v1)
        is Content.Heading ->
            Spans(
                c.spans,
                when (c.level.toInt()) {
                    1 -> Type.sheet
                    2 -> Type.header
                    else -> Type.body.copy(fontWeight = FontWeight.SemiBold)
                },
                Modifier.padding(top = 4.dp).semantics { heading() },
            )
        is Content.Code -> CodeBlock(c.text)
        is Content.Quote -> {
            val bar = pi.surfaceStrong
            CompositionLocalProvider(LocalContentColor provides pi.textSecondary) {
                Column(
                    Modifier.drawBehind {
                            drawRoundRect(
                                bar,
                                size = Size(3.dp.toPx(), size.height),
                                cornerRadius = CornerRadius(1.5.dp.toPx()),
                            )
                        }
                        .padding(start = 14.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    c.v1.forEach { BlockView(it) }
                }
            }
        }
        is Content.BlockList ->
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                c.items.forEachIndexed { i, item ->
                    val checked = (item.content as? Content.Item)?.checked
                    Row {
                        // 印は 1 行目の高さの中央に置く。
                        Box(
                            Modifier.widthIn(min = 20.dp)
                                .heightIn(min = with(LocalDensity.current) { 21.sp.toDp() })
                                .padding(end = 6.dp),
                            Alignment.CenterStart,
                        ) {
                            when {
                                checked != null -> TaskMark(checked)
                                c.start != null ->
                                    Text("${c.start.toLong() + i}.", color = pi.textSecondary)
                                else -> Dot(pi.text, 5.dp, Modifier.padding(start = 5.dp))
                            }
                        }
                        Box(Modifier.weight(1f)) { BlockView(item) }
                    }
                }
            }
        is Content.Item ->
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                c.children.forEach { BlockView(it) }
            }
        is Content.Table -> {
            var open by remember { mutableStateOf(false) }
            // 会話の中ではセルを長押しで選択しない。タップで全画面を開き、そこで選択・コピーする。
            DisableSelection {
                TableView(
                    c.v1,
                    Modifier.clickable(onClickLabel = "表を全画面で開く", role = Role.Button) {
                        open = true
                    },
                )
            }
            if (open) TableViewer(c.v1) { open = false }
        }
        Content.Rule -> Box(Modifier.fillMaxWidth().height(1.dp).background(pi.surfaceStrong))
    }
}

/** タスクリストの印。色だけでなく読み上げでも状態を伝える。 */
@Composable
private fun TaskMark(checked: Boolean) {
    val state = if (checked) "完了" else "未完了"
    Box(
        Modifier.size(14.dp)
            .then(
                if (checked) Modifier.background(pi.accent, RoundedCornerShape(4.dp))
                else Modifier.border(1.5.dp, pi.textTertiary, RoundedCornerShape(4.dp))
            )
            .semantics { contentDescription = state },
        Alignment.Center,
    ) {
        if (checked) Icon(Icons.Check, null, Modifier.size(11.dp), pi.onAccent)
    }
}

@Composable
private fun CodeBlock(code: String) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) {
        if (copied) {
            delay(1500)
            copied = false
        }
    }
    Box(Modifier.fillMaxWidth().background(pi.surface, RoundedCornerShape(10.dp))) {
        // タブは半角 2 つで表示する。元のタブが要るときは右上のコピーを使う。
        Text(
            code.trimEnd('\n').replace("\t", "  "),
            Modifier.horizontalScroll(rememberScrollState())
                .padding(start = 12.dp, top = 12.dp, end = tap, bottom = 12.dp),
            color = pi.textCode,
            style = Type.mono,
            softWrap = false,
        )
        IconAction(
            if (copied) Icons.Check else Icons.Copy,
            if (copied) "コピーしました" else "コードをコピー",
            {
                clipboard.setText(AnnotatedString(code))
                copied = true
            },
            Modifier.align(Alignment.TopEnd).background(pi.surface, RoundedCornerShape(10.dp)),
            size = 16.dp,
        )
    }
}

/**
 * 列幅は中核の table_widths（TS 版と同じ決め方）: 自然幅は上限 240dp、収まれば余りを配って両端まで広げ、 収まらなければ短い列を固定して長い列を最小 100dp
 * まで折り返し、それでも入らなければ自然幅のまま横に送る。
 */
@Composable
private fun TableBody(table: Table, available: Dp, modifier: Modifier = Modifier, scroll: Boolean) {
    val natural =
        remember(table) {
            List(table.header.size) { col ->
                (listOf(table.header) + table.rows).maxOf { row ->
                    row.getOrNull(col)?.let(::cellWidth) ?: 0u
                }
            }
        }
    val ratio = LocalDensity.current.fontScale.toDouble()
    val widths =
        remember(natural, available, ratio) {
            tableWidths(natural, available.value.toDouble(), ratio)
        }
    val total = widths.sum().dp
    // 収まる表は横送りにしない（表の上から右へ送っても一覧を開ける）。
    Column(
        if (scroll && total > available + .5.dp) modifier.horizontalScroll(rememberScrollState())
        else modifier
    ) {
        (listOf(table.header) + table.rows).forEachIndexed { index, row ->
            if (index > 1) Box(Modifier.width(total).height(1.dp).background(pi.surface))
            Row(
                if (index == 0) Modifier.background(pi.surface, RoundedCornerShape(8.dp))
                else Modifier
            ) {
                widths.forEachIndexed { col, width ->
                    Spans(
                        row.getOrElse(col) { emptyList() },
                        if (index == 0) Type.body.copy(fontWeight = FontWeight.SemiBold)
                        else Type.body,
                        Modifier.width(width.dp).padding(8.dp),
                        when (table.align.getOrNull(col)) {
                            uniffi.pimate_ffi.Alignment.CENTER -> TextAlign.Center
                            uniffi.pimate_ffi.Alignment.RIGHT -> TextAlign.End
                            else -> null
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun TableView(table: Table, modifier: Modifier = Modifier) {
    BoxWithConstraints(Modifier.fillMaxWidth()) { TableBody(table, maxWidth, modifier, true) }
}

/** 表の全画面。セルの文字は OS の選択で部分コピーでき、右上で表全体を Markdown としてコピーする。 */
@Composable
private fun TableViewer(table: Table, close: () -> Unit) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    Dialog(
        close,
        DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        // ステータスバーの帯を暗くしない（下の画面と同じ背景色がそのまま見える）。
        val window = (LocalView.current.parent as? DialogWindowProvider)?.window
        SideEffect { window?.setDimAmount(0f) }
        Column(Modifier.fillMaxSize().background(pi.background).safeDrawingPadding()) {
            Row(
                Modifier.height(52.dp).padding(horizontal = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconAction(Icons.Close, "閉じる", close, tint = pi.text)
                Text("表", Modifier.weight(1f).semantics { heading() }, style = Type.header)
                IconAction(
                    if (copied) Icons.Check else Icons.Copy,
                    if (copied) "コピーしました" else "表を Markdown でコピー",
                    {
                        clipboard.setText(AnnotatedString(table.markdown))
                        copied = true
                    },
                    size = 20.dp,
                )
            }
            BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
                val available = maxWidth - gutter * 2
                SelectionContainer {
                    Box(
                        Modifier.verticalScroll(rememberScrollState())
                            .horizontalScroll(rememberScrollState())
                            .padding(horizontal = gutter, vertical = 8.dp)
                    ) {
                        TableBody(table, available, scroll = false)
                    }
                }
            }
        }
    }
}
