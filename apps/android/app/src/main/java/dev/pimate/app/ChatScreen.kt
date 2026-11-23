package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import org.json.*
import uniffi.pimate_ffi.*

/** JSON の解析を行ごとに 1 回で済ませる（長い履歴に 1 件足すたびに全件を読み直さない）。 */
private class Parsed<T>(private val parse: (Item) -> T) {
    private var seen = emptyMap<String, Pair<String, T>>()

    fun of(items: List<Item>): List<T> {
        val next = HashMap<String, Pair<String, T>>(items.size)
        return items
            .map { i ->
                (seen[i.id]?.takeIf { it.first == i.json } ?: (i.json to parse(i)))
                    .also { next[i.id] = it }
                    .second
            }
            .also { seen = next }
    }
}

/** チャット（docs/design/html/Main.html・Chat-Waiting.html）。見出し・会話・入力欄。 */
@Composable
internal fun ChatScreen(m: SessionModel, openDrawer: () -> Unit) {
    val groups = m.rows.groups
    val view = groups["view"].orEmpty().associate { it.id to it.json }
    val target = m.target
    val active = view["agentActive"] == "true"
    val questions = groups["question"].orEmpty()
    val entryCache = remember(m.selected) { Parsed { entry(JSONObject(it.json)) } }
    val liveCache = remember(m.selected) { Parsed { live(it.id, JSONObject(it.json)) } }
    val entries = remember(groups["entry"]) { entryCache.of(groups["entry"].orEmpty()) }
    val tools = remember(groups["tool"]) { liveCache.of(groups["tool"].orEmpty()) }
    fun stream(id: String) =
        groups["stream"]
            .orEmpty()
            .firstOrNull { it.id == id }
            ?.let { JSONTokener(it.json).nextValue() as? String }
            .orEmpty()
    val text = stream("text")
    val thinking = stream("thinking")
    val rows =
        remember(entries, tools, active, text, thinking) {
            buildRows(entries, tools, active, text, thinking)
        }
    // 開いた要約・ツール・考えた内容の key。回転では保ち、Pi を切り替えたら閉じる。
    val open =
        rememberSaveable(
            m.selected,
            saver =
                listSaver<SnapshotStateList<String>, String>(
                    { it.toList() },
                    { it.toMutableStateList() },
                ),
        ) {
            mutableStateListOf()
        }
    val toggle = { key: String -> if (!open.remove(key)) open.add(key) }

    Column(Modifier.fillMaxSize().background(pi.background).safeDrawingPadding()) {
        Header(m, view, active, questions.isNotEmpty(), openDrawer)
        val phase = view["phase"]
        when {
            m.selected == null -> Placeholder("一覧から Pi を選ぶと、ここに会話が表示されます", "一覧を開く", openDrawer)
            phase == "\"instance_gone\"" -> Placeholder("この Pi は終了しました", "一覧を開く", openDrawer)
            phase == "\"subscribe_error\"" -> Placeholder("この Pi を開けませんでした", "一覧を開く", openDrawer)
            target == null -> Placeholder("読み込み中…")
            rows.isEmpty() && questions.isEmpty() -> Placeholder("メッセージを送ると、ここに会話が表示されます")
            else -> {
                val visible = rows.filter { it.fold == null || it.fold in open }
                // 逆順のリスト。先頭（最下段）の固定の項目が見えている間は、増えた行のぶん上へ伸びる。
                LazyColumn(
                    Modifier.weight(1f),
                    reverseLayout = true,
                    contentPadding = PaddingValues(start = gutter, top = 8.dp, end = gutter),
                ) {
                    item("bottom") { Spacer(Modifier.height(10.dp)) }
                    if (view["pcPrompt"].let { it != null && it != "null" })
                        item("pcPrompt") {
                            Note(
                                "PC の Pi が操作を待っています。スマホには表示できない画面です。PC で操作してください。",
                                Modifier.padding(top = 24.dp),
                            )
                        }
                    items(questions.asReversed(), key = { "question:${it.id}" }) { item ->
                        val editor = m.edits.current
                        val edit = editor?.let { m.answers[it, item.id] }
                        if (edit != null && target != null)
                            Box(Modifier.padding(top = 24.dp)) {
                                QuestionCard(
                                    remember(item.json) { JSONObject(item.json) },
                                    edit,
                                    m.can(
                                        Operation.Answer(
                                            target,
                                            JSONObject()
                                                .put("requestId", item.id)
                                                .put("cancelled", true)
                                                .toString(),
                                        )
                                    ),
                                ) { answer ->
                                    m.answer(editor, item.id, edit, answer)
                                }
                            }
                    }
                    items(visible.size, key = { "row:" + visible[visible.size - 1 - it].key }) { n
                        ->
                        val index = visible.size - 1 - n
                        val row = visible[index]
                        Box(Modifier.padding(top = gap(visible.getOrNull(index - 1), row).dp)) {
                            RowView(row, row.key in open, toggle)
                        }
                    }
                }
            }
        }
        Composer(m, active, questions.isNotEmpty())
    }
}

@Composable
private fun Header(
    m: SessionModel,
    view: Map<String, String>,
    active: Boolean,
    asking: Boolean,
    openDrawer: () -> Unit,
) {
    val session = view["session"]?.takeIf { it != "null" }?.let(::JSONObject)
    val cwd = view["cwd"]?.let { JSONTokener(it).nextValue() as? String }.orEmpty()
    // 経過は、この画面で待機から作業に変わるのを見たときだけ数える（開始時刻は状態の層にない）。
    var since by rememberSaveable(m.selected) { mutableStateOf<Long?>(null) }
    var idleSeen by rememberSaveable(m.selected) { mutableStateOf(false) }
    val ready = m.target != null
    LaunchedEffect(active, ready) {
        if (!ready) return@LaunchedEffect
        if (!active) {
            since = null
            idleSeen = true
        } else if (idleSeen && since == null) since = System.currentTimeMillis()
    }
    val now by
        produceState(System.currentTimeMillis(), since) {
            while (since != null) {
                value = System.currentTimeMillis()
                delay(1000)
            }
        }
    Row(
        Modifier.height(52.dp).padding(horizontal = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconAction(Icons.Menu, "Instance 一覧を開く", openDrawer, tint = pi.text)
        Text(
            if (m.selected == null) "PiMate" else title(session, cwd),
            Modifier.weight(1f).padding(start = 2.dp).semantics { heading() },
            style = Type.header,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Row(
            Modifier.padding(start = 6.dp, end = 16.dp).semantics(mergeDescendants = true) {
                liveRegion = LiveRegionMode.Polite
            },
            Arrangement.spacedBy(6.dp),
            Alignment.CenterVertically,
        ) {
            val phase = view["phase"]
            when {
                m.selected == null ||
                    phase == "\"instance_gone\"" ||
                    phase == "\"subscribe_error\"" -> Unit
                !ready -> {
                    Spinner()
                    Text("同期中", color = pi.textSecondary, style = Type.caption)
                }
                asking || view["status"] == "\"waiting_input\"" -> {
                    Dot(pi.waiting, 7.dp)
                    Text(
                        "入力待ち",
                        color = pi.waitingText,
                        style = Type.caption,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
                active -> {
                    Spinner()
                    Text(
                        since?.let { "作業中 · ${elapsed(now - it)}" } ?: "作業中",
                        // 秒ごとの読み上げはしない。
                        Modifier.semantics { contentDescription = "作業中" },
                        pi.textSecondary,
                        style = Type.caption,
                    )
                }
            }
        }
    }
}

@Composable
private fun ColumnScope.Placeholder(
    text: String,
    action: String? = null,
    onAction: () -> Unit = {},
) {
    Column(
        Modifier.weight(1f).fillMaxWidth().padding(horizontal = 40.dp),
        Arrangement.Center,
        Alignment.CenterHorizontally,
    ) {
        Text(
            text,
            color = pi.textSecondary,
            style =
                Type.body.copy(
                    lineBreak = Type.phrase,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                ),
        )
        if (action != null) TextAction(action, onAction)
    }
}

@Composable
private fun Note(text: String, modifier: Modifier = Modifier, error: Boolean = false) {
    Text(text, modifier, if (error) pi.failedText else pi.textSecondary, style = Type.caption)
}

@Composable
private fun RowView(row: ChatRow, open: Boolean, toggle: (String) -> Unit) {
    when (row) {
        is ChatRow.User ->
            Box(Modifier.fillMaxWidth(), Alignment.CenterEnd) {
                val accent = SpanStyle(color = pi.accentText, fontFamily = Mono, fontSize = 13.sp)
                SelectionContainer {
                    Text(
                        // @パス（メンション）はアクセントの等幅で示す。
                        remember(row.text, accent) {
                            buildAnnotatedString {
                                append(row.text)
                                Regex("(?<![^\\s])@[\\w./~-]+").findAll(row.text).forEach {
                                    addStyle(accent, it.range.first, it.range.last + 1)
                                }
                            }
                        },
                        Modifier.widthIn(max = 280.dp)
                            .background(pi.selected, RoundedCornerShape(18.dp))
                            .padding(horizontal = 14.dp, vertical = 10.dp),
                        style = Type.body.copy(lineBreak = Type.phrase),
                    )
                }
            }
        is ChatRow.Summary ->
            Expander(
                row.text,
                open,
                { toggle(row.key) },
                Modifier.heightIn(min = 32.dp),
                wide = false,
            )
        is ChatRow.Thinking ->
            Column {
                Expander(
                    if (row.live) "考え中…" else "考えた",
                    open,
                    { toggle(row.key) },
                    Modifier.fillMaxWidth().heightIn(min = 32.dp),
                ) {
                    if (row.live) Spinner()
                    else Icon(Icons.Bulb, null, Modifier.size(13.dp), pi.textSecondary)
                }
                if (open)
                    SelectionContainer {
                        Text(
                            row.text.trim(),
                            Modifier.padding(start = 22.dp, top = 2.dp, bottom = 6.dp),
                            pi.textSecondary,
                            style = Type.caption,
                        )
                    }
            }
        is ChatRow.Text ->
            Column {
                Markdown(row.text, live = row.key == "live:text")
                if (row.meta.isNotEmpty())
                    Text(
                        row.meta,
                        Modifier.padding(top = 6.dp),
                        pi.textSecondary,
                        style = Type.meta,
                    )
            }
        is ChatRow.Tool -> ToolRow(row, open) { toggle(row.key) }
        is ChatRow.Note -> Note(row.text, error = row.error)
    }
}

/** 開閉できる 1 行（ターンの要約・考えた内容）。 */
@Composable
private fun Expander(
    label: String,
    open: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    wide: Boolean = true,
    mark: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier
            .clip(RoundedCornerShape(8.dp))
            .clickable(
                onClickLabel = if (open) "閉じる" else "開く",
                role = Role.Button,
                onClick = onClick,
            )
            .semantics { stateDescription = if (open) "開いています" else "閉じています" },
        Arrangement.spacedBy(if (mark == null) 6.dp else 8.dp),
        Alignment.CenterVertically,
    ) {
        if (mark != null) MarkColumn(18.dp) { mark() }
        Text(
            label,
            if (wide) Modifier.weight(1f) else Modifier,
            pi.textSecondary,
            style = Type.caption,
        )
        Icon(if (open) Icons.Up else Icons.Down, null, Modifier.size(14.dp), pi.textSecondary)
    }
}

/** ツール呼び出しの簡潔な行（32dp）。タップで出力を開く。 */
@Composable
private fun ToolRow(row: ChatRow.Tool, open: Boolean, onClick: () -> Unit) {
    val output =
        remember(row.output) {
            row.output
                .trimEnd()
                .lines()
                .let { if (it.size > 40) listOf("…") + it.takeLast(40) else it }
                .joinToString("\n")
        }
    val canOpen = output.isNotEmpty() || row.state == ToolState.Running
    Column {
        Row(
            Modifier.fillMaxWidth()
                .heightIn(min = 32.dp)
                .clip(RoundedCornerShape(8.dp))
                .clickable(canOpen, if (open) "出力を閉じる" else "出力を開く", Role.Button, onClick)
                .semantics { stateDescription = row.state.label },
            Arrangement.spacedBy(8.dp),
            Alignment.CenterVertically,
        ) {
            MarkColumn(18.dp) {
                when (row.state) {
                    ToolState.Running -> Spinner()
                    ToolState.Failed -> Dot(pi.failed)
                    ToolState.Done -> Dot(pi.done)
                }
            }
            Text(row.name, style = Type.mono.copy(fontWeight = FontWeight.SemiBold), maxLines = 1)
            Text(
                row.subject,
                Modifier.weight(1f),
                pi.textSecondary,
                style = Type.mono,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (row.state == ToolState.Failed)
                Text(row.meta, color = pi.failedText, style = Type.meta)
            else if (row.meta.startsWith("+"))
                row.meta.split(" ").forEach {
                    Text(
                        it,
                        color = if (it.startsWith("+")) pi.addText else pi.delText,
                        style = Type.meta,
                    )
                }
            else if (row.meta.isNotEmpty())
                Text(row.meta, color = pi.textSecondary, style = Type.meta)
            if (canOpen)
                Icon(
                    if (open) Icons.Up else Icons.Down,
                    null,
                    Modifier.size(14.dp),
                    pi.textSecondary,
                )
        }
        if (open && canOpen)
            SelectionContainer {
                Text(
                    output.ifEmpty { "出力を待っています…" },
                    Modifier.padding(start = 22.dp, top = 4.dp, bottom = 4.dp)
                        .fillMaxWidth()
                        .background(pi.surface, RoundedCornerShape(10.dp))
                        .horizontalScroll(rememberScrollState())
                        .padding(horizontal = 12.dp, vertical = 10.dp),
                    pi.textCode,
                    style = Type.mono.copy(fontSize = 11.sp),
                    softWrap = false,
                )
            }
    }
}

/** 入力欄と、その上の送信待ち。受け付けられただけの本文は履歴に混ぜず、ここに分けて出す。 */
@Composable
private fun Composer(m: SessionModel, active: Boolean, asking: Boolean) {
    val target = m.target
    val editor = m.edits.current
    val body = m.edits.body
    val view = m.rows.groups["view"].orEmpty().associate { it.id to it.json }
    val drafts =
        m.rows.groups["draft"].orEmpty().mapNotNull { item ->
            val d = JSONObject(item.json)
            Triple(item.id, d, d.str("status")).takeIf {
                d.str("instanceId") == m.selected &&
                    it.third in listOf("queued", "unconfirmed", "sending")
            }
        }
    Column(Modifier.padding(start = 16.dp, top = 6.dp, end = 16.dp, bottom = 8.dp)) {
        if (drafts.isNotEmpty())
            Column(
                Modifier.heightIn(max = 200.dp)
                    .verticalScroll(rememberScrollState())
                    .padding(bottom = 6.dp)
            ) {
                drafts.forEach { (id, d, status) ->
                    val text = d.optJSONObject("params")?.str("text").orEmpty()
                    if (status == "unconfirmed") {
                        val restorable =
                            !m.busy &&
                                body.text.isEmpty() &&
                                body.images.isEmpty() &&
                                target != null &&
                                target.session == (d.opt("sessionId") as? String)
                        Column(
                            Modifier.fillMaxWidth()
                                .padding(bottom = 6.dp)
                                .background(pi.surface, RoundedCornerShape(16.dp))
                                .padding(start = 14.dp, top = 12.dp, end = 4.dp, bottom = 2.dp)
                        ) {
                            Text(
                                "届いていない可能性があります",
                                color = pi.waitingText,
                                style = Type.caption,
                                fontWeight = FontWeight.SemiBold,
                            )
                            Text(
                                "PC の入力欄に戻っている場合があります。履歴を確認してください。",
                                Modifier.padding(end = 10.dp),
                                pi.textSecondary,
                                style = Type.caption,
                            )
                            Text(
                                text,
                                Modifier.padding(top = 6.dp, end = 10.dp),
                                maxLines = 3,
                                overflow = TextOverflow.Ellipsis,
                            )
                            TextAction(
                                "下書きへ戻す",
                                { editor?.let { m.restore(it, id) } },
                                Modifier.offset(x = (-14).dp),
                                enabled = restorable,
                            )
                        }
                    } else
                        Row(
                            Modifier.fillMaxWidth()
                                .heightIn(min = 40.dp)
                                .padding(start = 12.dp, end = 12.dp)
                                .semantics(mergeDescendants = true) {},
                            Arrangement.spacedBy(8.dp),
                            Alignment.CenterVertically,
                        ) {
                            Icon(Icons.Reply, null, Modifier.size(14.dp), pi.textSecondary)
                            Text(
                                if (status == "queued") "割り込み待ち" else "送信中",
                                color = pi.textSecondary,
                                style = Type.caption,
                            )
                            Text(
                                text,
                                Modifier.weight(1f),
                                style = Type.body.copy(fontSize = 13.sp),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                }
            }
        if (m.error.isNotEmpty())
            Text(
                m.error,
                Modifier.padding(start = 12.dp, end = 12.dp, bottom = 6.dp).semantics {
                    liveRegion = LiveRegionMode.Polite
                },
                pi.failedText,
                style = Type.caption,
            )
        Column(
            Modifier.background(pi.surface, RoundedCornerShape(24.dp))
                .padding(start = 14.dp, top = 12.dp, end = 8.dp, bottom = 6.dp)
        ) {
            if (body.images.isNotEmpty())
                Text(
                    "添付画像 ${body.images.size} 枚",
                    Modifier.padding(bottom = 4.dp),
                    pi.textSecondary,
                    style = Type.caption,
                )
            val hint =
                when {
                    target == null -> "Pi を選ぶと入力できます"
                    asking -> "上の質問に回答すると入力できます"
                    else -> "メッセージ"
                }
            // 入力は ViewModel（ChatEdits）が持つ。欄は編集の対象ごとに作り直し、古い IME の確定を捨てる。
            key(editor) {
                BasicTextField(
                    body.text,
                    { text -> editor?.let { m.edits.text(it, text) } },
                    Modifier.fillMaxWidth().padding(end = 6.dp).semantics {
                        contentDescription = "メッセージ"
                    },
                    enabled = target != null && !asking,
                    textStyle = Type.lead.copy(color = pi.text),
                    maxLines = 5,
                    cursorBrush = SolidColor(pi.accent),
                    decorationBox = { inner ->
                        if (body.text.isEmpty())
                            Text(
                                hint,
                                color = pi.textSecondary,
                                style = Type.lead,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        inner()
                    },
                )
            }
            Row(
                Modifier.padding(top = 6.dp).heightIn(min = 36.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // 左は状態の 1 行。モデルの選択と使用状況のリング（M3b）はこの位置に入る。
                val model =
                    view["model"]
                        ?.takeIf { it != "null" }
                        ?.let { JSONObject(it).str("id") }
                        .orEmpty()
                val level =
                    view["thinking"]?.let { JSONTokener(it).nextValue() as? String }.orEmpty()
                Text(
                    if (active) "割り込みで送信"
                    else listOf(model, level).filter { it.isNotEmpty() }.joinToString(" · "),
                    Modifier.weight(1f).padding(end = 8.dp),
                    pi.textSecondary,
                    style = Type.body.copy(fontSize = 13.sp),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (active && target != null) {
                    val stop = m.can(Operation.Abort(target))
                    Box(
                        Modifier.size(36.dp).clip(CircleShape).clickable(stop, role = Role.Button) {
                            m.operate(Operation.Abort(target))
                        },
                        Alignment.Center,
                    ) {
                        Icon(
                            Icons.Stop,
                            "停止",
                            Modifier.size(22.dp),
                            pi.text.copy(alpha = if (stop) 1f else .4f),
                        )
                    }
                    Spacer(Modifier.width(2.dp))
                }
                val send =
                    (body.text.isNotBlank() || body.images.isNotEmpty()) &&
                        m.can(target?.let { Operation.Send(it, body.text, body.images) })
                Box(
                    Modifier.size(36.dp)
                        .clip(CircleShape)
                        .background(if (send) pi.accent else pi.surfaceStrong)
                        .clickable(send, role = Role.Button) { editor?.let { m.send(it, body) } },
                    Alignment.Center,
                ) {
                    Icon(
                        Icons.Send,
                        if (active) "割り込む" else "送信",
                        Modifier.size(18.dp),
                        if (send) pi.onAccent else pi.textSecondary,
                    )
                }
            }
        }
    }
}
