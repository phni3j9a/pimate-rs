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
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.*
import androidx.compose.ui.text.font.*
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.*
import kotlinx.coroutines.*
import uniffi.pimate_ffi.*

/** 解析した回答: 描画する block と、表示順の画像（全画面で順に送る）。 */
private class Doc(val blocks: List<Block>, val pics: List<Pic>)

private val empty = Doc(emptyList(), emptyList())

// files: PC のパスを画像として扱う（file_image が使える Pi だけ。使えなければ文字のまま）。
private fun doc(source: String, files: Boolean) =
    parse(source, files).use { d ->
        Doc(d.blocks(), d.images().map { Pic.Shared(it.kind, it.src) })
    }

// 解析済みの回答。スクロールで戻った回答を解析し直さずにすぐ出す。
private val parsed = android.util.LruCache<Pair<Boolean, String>, Doc>(64)
// 逐次表示の最後の解析（増えるたびに上のものを追い出さないよう分ける）。確定した回答が同じ本文ならそのまま使う。
private var lastLive: Triple<Boolean, String, Doc>? = null

private fun cached(source: String, files: Boolean) =
    parsed[files to source] ?: lastLive?.takeIf { it.first == files && it.second == source }?.third

// 逐次表示から確定した直後は、解析が済むまで逐次表示の最後の解析を出す（回答が 1 フレーム消えない）。
private fun initial(source: String, files: Boolean) =
    cached(source, files)
        ?: lastLive
            ?.takeIf {
                it.first == files &&
                    source.startsWith(it.second) &&
                    it.second.length * 5 >= source.length * 4
            }
            ?.third
        ?: empty

/** 回答の中の画像（表示順）。タイルから全画面を開くときに、同じ回答の画像をまとめて渡す。 */
private val LocalDocPics = compositionLocalOf<List<Pic>> { emptyList() }

/** 回答の本文。長押しで OS の選択になり、範囲を広げてコピーできる（選択は 1 つの回答の中）。 解析は UI スレッドの外で行い、live（逐次表示）は前の表示を残したまま差し替える。 */
@Composable
internal fun Markdown(
    source: String,
    modifier: Modifier = Modifier,
    live: Boolean = false,
    answer: String? = null,
) {
    val files = LocalPics.current.files
    val shown by
        produceState(initial(source, files), source, files) {
            value = cached(source, files) ?: withContext(Dispatchers.Default) { doc(source, files) }
            if (live) lastLive = Triple(files, source, value)
            else parsed.put(files to source, value)
        }
    val gallery =
        remember(shown, answer) {
            if (answer == null) shown.pics
            else shown.pics.map { if (it is Pic.Shared) it.forAnswer(answer) else it }
        }
    CompositionLocalProvider(LocalDocPics provides gallery) {
        SelectionContainer(modifier) {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                shown.blocks.forEach { key(it.id) { BlockView(it) } }
            }
        }
    }
}

private const val CODE = "code"

/** links: false ならリンクは見た目だけ（タップは親の操作に渡す）。 */
private fun annotate(spans: List<Inline>, p: Palette, links: Boolean) = buildAnnotatedString {
    spans.forEach { span ->
        when (span) {
            is Inline.Image -> append(span.v1.alt.ifEmpty { "画像" })
            is Inline.Text -> {
                val s = span.style
                // href は中核が http(s) だけに絞っている。
                val url = s.href?.value()
                val look =
                    SpanStyle(color = p.accentText, textDecoration = TextDecoration.Underline)
                if (url != null)
                    if (links) pushLink(LinkAnnotation.Url(url, TextLinkStyles(look)))
                    else pushStyle(look)
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
    links: Boolean = true,
) {
    val p = pi
    val text = remember(spans, p, links) { annotate(spans, p, links) }
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    Text(
        text,
        modifier.drawBehind { layout?.let { chips(it, text, p.surface) } },
        style = style,
        textAlign = align,
        onTextLayout = { layout = it },
    )
}

/** 段落。画像は文字の途中で分け、出てくる順にタイルで置く（タップで、その回答の画像を全画面で送って見られる）。 */
@Composable
private fun Paragraph(
    spans: List<Inline>,
    style: TextStyle = LocalTextStyle.current,
    modifier: Modifier = Modifier,
    align: TextAlign? = null,
    links: Boolean = true,
) {
    if (spans.none { it is Inline.Image }) return Spans(spans, style, modifier, align, links)
    val pics = LocalPics.current
    val all = LocalDocPics.current
    // 画像で区切った文字の列と、画像を、元の順に並べる。
    val parts =
        remember(spans) {
            val out = mutableListOf<Any>()
            var run = mutableListOf<Inline>()
            for (span in spans) {
                if (span is Inline.Image) {
                    if (run.any { it is Inline.Text && it.text.isNotBlank() }) out.add(run)
                    run = mutableListOf()
                    out.add(span.v1)
                } else run += span
            }
            if (run.any { it is Inline.Text && it.text.isNotBlank() }) out.add(run)
            out
        }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        parts.forEach { part ->
            if (part is MarkdownImage) {
                val index = part.index.toInt()
                // 選択の対象にしない（長押しはタイルの上では何もしない）。
                DisableSelection {
                    PicTile(
                        all[index],
                        part.alt.ifEmpty { "画像" },
                        compact = false,
                        { if (index in all.indices) pics.open(all, index) },
                    )
                }
            } else {
                @Suppress("UNCHECKED_CAST")
                Spans(part as List<Inline>, style, align = align, links = links)
            }
        }
    }
}

@Composable
private fun BlockView(block: Block) {
    when (val c = block.content) {
        is Content.Paragraph -> Paragraph(c.v1)
        is Content.Heading ->
            Paragraph(
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
internal fun CopyAction(source: String, label: String) {
    val clipboard = LocalClipboardManager.current
    IconAction(Icons.Copy, label, { clipboard.setText(AnnotatedString(source)) }, size = 16.dp)
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
private fun TableBody(
    table: Table,
    available: Dp,
    modifier: Modifier = Modifier,
    scroll: Boolean,
    links: Boolean,
) {
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
    val cell = Type.caption.copy(fontSize = 13.sp, lineHeight = 19.sp)
    // 収まる表は横送りにしない（表の上から右へ送っても一覧を開ける）。
    Column(
        modifier
            .background(pi.surface, RoundedCornerShape(10.dp))
            .then(
                if (scroll && total > available + .5.dp)
                    Modifier.horizontalScroll(rememberScrollState())
                else Modifier
            )
    ) {
        (listOf(table.header) + table.rows).forEachIndexed { index, row ->
            if (index > 0) Box(Modifier.width(total).height(1.dp).background(pi.background))
            Row {
                widths.forEachIndexed { col, width ->
                    Paragraph(
                        row.getOrElse(col) { emptyList() },
                        if (index == 0) cell.copy(fontWeight = FontWeight.SemiBold) else cell,
                        Modifier.width(width.dp).padding(8.dp),
                        when (table.align.getOrNull(col)) {
                            uniffi.pimate_ffi.Alignment.CENTER -> TextAlign.Center
                            uniffi.pimate_ffi.Alignment.RIGHT -> TextAlign.End
                            else -> null
                        },
                        links,
                    )
                }
            }
        }
    }
}

@Composable
private fun TableView(table: Table, modifier: Modifier = Modifier) {
    // 会話の中ではセルのリンクを開かず、タップは全画面を開く。リンクは全画面で開く。
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        TableBody(table, maxWidth, modifier, scroll = true, links = false)
    }
}

/** 表の全画面。セルの文字は OS の選択で部分コピーでき、右上で表全体を Markdown としてコピーする。 */
@Composable
private fun TableViewer(table: Table, close: () -> Unit) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    FullScreen(close) {
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
                        TableBody(table, available, scroll = false, links = true)
                    }
                }
            }
        }
    }
}
