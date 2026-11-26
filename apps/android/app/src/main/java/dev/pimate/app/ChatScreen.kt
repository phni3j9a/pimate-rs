package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
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

/**
 * チャットの画面の一時的な状態。回転はまたぐが、保存はしない（全画面の画像は大きくて SavedState に入らない）。 接続・下書き・回答は持たない（それらは
 * SessionModel）。対象は、開いた瞬間の EditorKey で覚える。
 */
internal class ChatUi : ViewModel() {
    // 最新の受領結果だけをメモリに置く。別の Pi のバナーや入力欄には渡さない。
    private var received by mutableStateOf<ReceivedResult?>(null)

    fun receive(host: String?, instance: String?, result: JSONObject?, receipt: Receipt?) {
        if (host == null || instance == null || result == null) return
        val id = result.str("id")
        if (receipt?.take(id, instance) == true) received = ReceivedResult(host, instance, result)
        else resultFor(host, instance)?.let {
            if (it.result.str("id") == id) it.update(result)
            else received = null // Host の latest が更新された。古い案内・入力文を適用しない。
        }
    }

    fun resultFor(host: String?, instance: String?) =
        received?.takeIf { it.host == host && it.instance == instance }

    /** 開いているシート・画面（model / usage / session / sessions / fork / tree）と、開いた瞬間の対象。 */
    var sheet by mutableStateOf<Pair<String, EditorKey>?>(null)

    /** 全画面で開いている画像と、開いた瞬間の対象（会話の外の画像なら null）。 */
    var viewer by mutableStateOf<Triple<EditorKey?, List<Pic>, Int>?>(null)

    /** フォトピッカーを開いた瞬間の対象。結果が返るまで覚えておく。 */
    var picking: EditorKey? = null

    /** 選んだ写真を縮小している間。 */
    var encoding by mutableStateOf(false)

    /** 添付や / コマンドの操作で起きたこと（入力欄の上で案内する）。失敗なら赤。 */
    var problem by mutableStateOf<Triple<String, String, Boolean>?>(null)

    /** 書きかけの本文がある対象（中身は持たない）。切り替えで下書きが元の会話に残ったことを伝えるのに使う。 */
    val drafted = mutableSetOf<EditorKey>()

    /** 「送信先が変わりました」を確認した移動先と通知。 */
    var confirmed by mutableStateOf<SwitchId?>(null)

    /** 切り替え（from>to）を最初に見たときの、履歴の最後の発言。切り替えの行をその後ろに置く。 */
    var switched by mutableStateOf<Pair<SwitchId, String?>?>(null)
}

internal data class SwitchId(val target: EditorKey, val move: ChatMove)

private fun EditorKey.scope() = "$host/${target.instance}/${target.session}"

internal fun originalRunsByToolCall(runs: List<JSONObject>) = buildMap {
    for (run in runs) run.str("toolCallId").takeIf(String::isNotEmpty)?.let { putIfAbsent(it, run) }
}

/**
 * チャット（docs/design/html/Main.html・Chat-Waiting.html・Chat-Edge.html・Chat-Attach.html）。見出し・会話・入力欄と、
 * そこから開くシート・全画面の画像。
 */
@Composable
internal fun ChatScreen(
    m: SessionModel,
    ui: ChatUi,
    openDrawer: () -> Unit,
    recover: (Stop) -> Unit,
) {
    val groups = m.rows.groups
    val view = groups["view"].orEmpty().associate { it.id to it.json }
    val target = m.target
    val editor = m.edits.current
    // 操作を送れる対象（表示中の会話が読み込み済みで、編集の対象と同じ）。
    val key = editor?.takeIf { target != null && it.target == target }
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
    val thinkingMs =
        remember(groups["thinkingMs"]) {
            groups["thinkingMs"].orEmpty().associate { it.id to (it.json.toLongOrNull() ?: 0) }
        }
    val rows =
        remember(entries, tools, active, text, thinking, thinkingMs) {
            buildRows(entries, tools, active, text, thinking, thinkingMs)
        }
    // 利用者が開閉した行: 「+key」は開いた、「-key」は閉じた。回転では保ち、Pi を切り替えたら忘れる。
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
    // 実行中のツールと、考えている途中の内容は開いておく（終わると閉じる）。利用者が開閉したら、それに従う。
    fun isOpen(row: ChatRow) =
        when {
            "+${row.key}" in open -> true
            "-${row.key}" in open -> false
            else ->
                row is ChatRow.Tool && row.state == ToolState.Running ||
                    row is ChatRow.Thinking && row.live && row.text.isNotBlank()
        }
    val toggle = { row: ChatRow ->
        val was = isOpen(row)
        open.remove("+${row.key}")
        open.remove("-${row.key}")
        open.add((if (was) "-" else "+") + row.key)
        Unit
    }

    val connection = groups["host"].orEmpty().find { it.id == "connection" }?.json
    val stop = stopped(connection)
    val offline = connection != "\"connected\"" && stop == null
    val pcPrompt = view["pcPrompt"]?.takeIf { it != "null" }?.let(::JSONObject)
    val session = view["session"]?.takeIf { it != "null" }?.let(::JSONObject)

    // 画像の取得は、いま表示している対象に結び付ける。対象が変わったあとに返った結果は SessionModel.request が捨てる。
    val pics =
        remember(editor, view["entryImages"], view["fileImages"]) {
            Pics(
                editor?.scope().orEmpty(),
                view["entryImages"] == "true",
                view["fileImages"] == "true",
                { method, params ->
                    m.ask(editor ?: throw CoreException.StaleTarget(), method, params)
                },
            ) { list, index ->
                ui.viewer = Triple(editor, list, index)
            }
        }

    // 会話を開いたら、/ コマンドとモデルの一覧を取りに行く（再同期で消えるので、読み込み済みになるたび）。
    var commandsFailed by remember(key) { mutableStateOf(false) }
    var commandsTry by remember(key) { mutableIntStateOf(0) }
    LaunchedEffect(key, commandsTry) {
        if (key == null) return@LaunchedEffect
        if (view["slashCommands"] == "true")
            commandsFailed =
                try {
                    m.ask(key, RcMethod.COMMANDS, JSONObject())
                    false
                } catch (_: CoreException) {
                    true
                }
        if (commandsTry == 0)
            try {
                m.ask(key, RcMethod.MODELS, JSONObject())
            } catch (_: CoreException) {
                // 名前が分からなければ、入力欄には ID をそのまま出す。
            }
    }

    // ---- セッションの切り替え（PC の /resume など）と、操作の結果 ----
    val changed =
        groups["notice"].orEmpty().find { it.id == "sessionChanged" }?.let { JSONObject(it.json) }
    val busyOp = groups["busy"].orEmpty().find { it.id == "operation" }?.let { JSONObject(it.json) }
    val latest = groups["operationResult"].orEmpty().find { it.id == "latest" }
    val hostId = m.host?.id
    val instance = m.selected
    val delivered = ui.resultFor(hostId, instance)
    val receipt = m.rows.notices.receipt
    val latestResult = remember(latest) { latest?.let { JSONObject(it.json) } }
    SideEffect {
        ui.receive(hostId, instance, latestResult, receipt)
        if (key != null && key == m.edits.current && key.target == m.target)
            ui.resultFor(hostId, instance)
                ?.takeText(key, m.edits.body.text.isEmpty() && m.edits.body.images.isEmpty())
                ?.let { m.edits.text(key, it) }
    }
    val fromId = changed?.optJSONObject("from")?.str("id").orEmpty()
    val toId = changed?.optJSONObject("to")?.str("id").orEmpty()
    val move = m.rows.notices.move
    val switchId = if (key != null && move != null) SwitchId(key, move) else null
    val byPhone = move?.byPhone == true
    // 書きかけがある対象を覚える（中身は ChatEdits が持つ）。
    val body = m.edits.body
    SideEffect {
        if (editor != null)
            if (body.text.isNotEmpty() || body.images.isNotEmpty()) ui.drafted += editor
            else ui.drafted -= editor
    }
    // 切り替え前の会話に下書きが残った: 送信先が変わったことを確かめてもらうまで、送らせない。
    val left =
        editor != null &&
            changed != null &&
            !byPhone &&
            session?.str("id") == toId &&
            switchId != null &&
            ui.confirmed != switchId &&
            editor.copy(target = Target(editor.target.instance, fromId)) in ui.drafted
    // 対象が変わったら、前の対象のシートと画像は閉じる。
    val runLinks =
        remember(entries, groups["run"]) {
            val runs =
                groups["run"].orEmpty().map { JSONObject(it.json) }.let(::originalRunsByToolCall)
            buildMap {
                runs.forEach { (id, r) -> put("running:$id", r.str("id")) }
                entries.forEach { e ->
                    e.calls.forEach { c ->
                        runs[c.id]?.let { put("tool:${e.id}:${c.id}", it.str("id")) }
                    }
                }
            }
        }
    val sheet = ui.sheet?.takeIf { it.second == editor }
    val viewer = ui.viewer?.takeIf { it.first == null || it.first == editor }
    SideEffect {
        if (ui.sheet != null && sheet == null) ui.sheet = null
        if (ui.viewer != null && viewer == null) ui.viewer = null
    }
    val show = { name: String -> if (editor != null) ui.sheet = name to editor }

    CompositionLocalProvider(LocalPics provides pics) {
        Column(Modifier.fillMaxSize().background(pi.background).safeDrawingPadding()) {
            Header(
                m,
                view,
                active,
                questions.isNotEmpty(),
                stop != null,
                offline,
                pcPrompt != null,
                openDrawer,
                { show("session") }.takeIf { key != null },
            )
            val notice =
                notice(
                    m.selected != null,
                    view["phase"],
                    target != null,
                    // 空の会話でも、切り替わったことは会話の欄に出す。
                    rows.isNotEmpty() || changed != null && session?.str("id") == toId,
                    questions.isNotEmpty(),
                    pcPrompt != null,
                    offline,
                )
            when {
                stop != null -> Placeholder(stop.detail, stop.action) { recover(stop) }
                notice != null ->
                    Placeholder(notice.first, "一覧を開く".takeIf { notice.second }, openDrawer)
                else -> {
                    val shown = rows.filter { it.fold == null || "+${it.fold}" in open }
                    // 会話が切り替わったことは、切り替えを見た位置（その時点の履歴の後ろ）に残す。
                    val here = changed != null && session?.str("id") == toId
                    SideEffect {
                        if (here && switchId != null && ui.switched?.first != switchId)
                            ui.switched = switchId to entries.lastOrNull()?.id
                    }
                    val visible =
                        if (!here) shown
                        else
                            placeSwitch(
                                shown,
                                ChatRow.Switch(
                                    changed!!.str("reason"),
                                    byPhone,
                                    changed.optJSONObject("from")?.str("name").orEmpty(),
                                    changed.optJSONObject("to")?.str("name").orEmpty().ifEmpty {
                                        entries.firstOrNull { it.kind == "user" }?.text.orEmpty()
                                    },
                                ),
                                entries,
                                ui.switched?.second,
                                ui.switched?.first == switchId,
                            )
                    val list = rememberLazyListState()
                    // 履歴を遡っている間に届いた質問は画面の外に足される。回答するまで入力できないので、カード（最下段）へ戻す。
                    val newest = questions.lastOrNull()?.id
                    LaunchedEffect(newest) { if (newest != null) list.animateScrollToItem(0) }
                    // 逆順のリスト。先頭（最下段）の固定の項目が見えている間は、増えた行のぶん上へ伸びる。
                    LazyColumn(
                        Modifier.weight(1f).testTag("chat"),
                        list,
                        reverseLayout = true,
                        contentPadding = PaddingValues(start = gutter, top = 8.dp, end = gutter),
                    ) {
                        item("bottom") { Spacer(Modifier.height(10.dp)) }
                        if (pcPrompt != null && questions.isEmpty())
                            item("pcPrompt") { PcCard(pcPrompt.str("title"), m.attachCommand()) }
                        items(questions.asReversed(), key = { "question:${it.id}" }) { item ->
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
                        items(
                            visible.size,
                            key = {
                                val id = visible[visible.size - 1 - it].key
                                // 別の逐次回答に切り替われば、同じ PC パスでもタイルを作り直す。
                                if (id == "live:text") "live:${entries.lastOrNull()?.id}"
                                else "row:$id"
                            },
                        ) { n ->
                            val index = visible.size - 1 - n
                            val row = visible[index]
                            Box(Modifier.padding(top = gap(visible.getOrNull(index - 1), row).dp)) {
                                RowView(
                                    row,
                                    if (row is ChatRow.Summary) "+${row.key}" in open
                                    else isOpen(row),
                                ) {
                                    if (row is ChatRow.Summary) {
                                        if (!open.remove("+${row.key}")) open.add("+${row.key}")
                                    } else if (runLinks.containsKey(row.key) && key != null)
                                        show("agents:" + runLinks.getValue(row.key))
                                    else toggle(row)
                                }
                            }
                        }
                    }
                }
            }
            val counts = remember(m.rows) { m.rows.subagentCounts() }
            if (counts.isNotEmpty())
                TextAction(
                    "サブエージェント  " +
                        counts.entries.joinToString(" · ") { "${it.value} ${agentState(it.key)}" },
                    { show("agents") },
                    enabled = key != null,
                )
            Composer(
                m,
                ui,
                key,
                active,
                questions.isNotEmpty(),
                pcPrompt != null,
                offline,
                left,
                changed?.let { c ->
                    c.optJSONObject("from")?.str("name").orEmpty() to
                        title(
                            session,
                            view["cwd"]?.let { JSONTokener(it).nextValue() as? String }.orEmpty(),
                        )
                },
                { ui.confirmed = switchId },
                busyOp,
                delivered?.takeUnless { it.dismissed }?.result,
                { delivered?.dismissed = true },
                commandsFailed,
                { commandsTry++ },
                show,
            )
        }
        sheet?.let { (name, at) ->
            val close = { ui.sheet = null }
            when (name) {
                "model" -> ModelSheet(m, at, close)
                "usage" -> UsageSheet(m, at, close)
                "tree" -> TreeScreen(m, at, close)
                "review" -> ReviewPage(m, at, close)
                "github" -> GithubPage(m, at, close)
                "agents" -> AgentsPage(m, at, close)
                else ->
                    if (name.startsWith("agents:"))
                        AgentsPage(m, at, close, name.removePrefix("agents:"))
                    else SessionSheet(m, at, name, { ui.sheet = it to at }, close)
            }
        }
        viewer?.let { (_, list, index) -> ImageViewer(list, index) { ui.viewer = null } }
    }
}

@Composable
private fun Header(
    m: SessionModel,
    view: Map<String, String>,
    active: Boolean,
    asking: Boolean,
    stopped: Boolean,
    offline: Boolean,
    pcPrompt: Boolean,
    openDrawer: () -> Unit,
    openMenu: (() -> Unit)?,
) {
    val session = view["session"]?.takeIf { it != "null" }?.let(::JSONObject)
    val cwd = view["cwd"]?.let { JSONTokener(it).nextValue() as? String }.orEmpty()
    // 作業の開始時刻は状態の層にある（この端末が見始めた時刻のこともある）。
    val since = view["runningSince"]?.toLongOrNull()?.takeIf { active }
    val ready = m.target != null
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
        IconButton(onClick = openDrawer, modifier = Modifier.semantics { contentDescription = "Instance 一覧を開く" }) {
            val color = pi.text
            Canvas(Modifier.size(24.dp)) {
                for (y in listOf(6.dp, 12.dp, 18.dp)) drawLine(color,
                    androidx.compose.ui.geometry.Offset(3.dp.toPx(), y.toPx()),
                    androidx.compose.ui.geometry.Offset(21.dp.toPx(), y.toPx()),
                    2.dp.toPx(), androidx.compose.ui.graphics.StrokeCap.Round)
            }
        }
        Text(
            if (m.selected == null) "PiMate" else title(session, cwd),
            Modifier.weight(1f).padding(start = 2.dp).semantics { heading() },
            style = Type.header,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Row(
            Modifier.padding(start = 6.dp, end = if (openMenu == null) 16.dp else 2.dp).semantics(
                mergeDescendants = true
            ) {
                liveRegion = LiveRegionMode.Polite
            },
            Arrangement.spacedBy(6.dp),
            Alignment.CenterVertically,
        ) {
            val phase = view["phase"]
            @Composable
            fun waiting(label: String) {
                Dot(pi.waiting, 7.dp)
                Text(
                    label,
                    color = pi.waitingText,
                    style = Type.caption,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                )
            }
            when {
                m.selected == null ||
                    phase == "\"instance_gone\"" ||
                    phase == "\"subscribe_error\"" ||
                    stopped -> Unit
                offline -> {
                    Spinner()
                    Text("再接続中", color = pi.textSecondary, style = Type.caption, maxLines = 1)
                }
                !ready -> {
                    Spinner()
                    Text("同期中", color = pi.textSecondary, style = Type.caption, maxLines = 1)
                }
                pcPrompt && !asking -> waiting("PC で操作待ち")
                asking || view["status"] == "\"waiting_input\"" -> waiting("入力待ち")
                active -> {
                    Spinner()
                    Text(
                        since?.let { "作業中 · ${elapsed((now - it).coerceAtLeast(0))}" } ?: "作業中",
                        // 秒ごとの読み上げはしない。
                        Modifier.semantics { contentDescription = "作業中" },
                        pi.textSecondary,
                        style = Type.caption,
                        maxLines = 1,
                    )
                }
                else -> {
                    Dot(pi.textTertiary, 6.dp)
                    Text("待機", color = pi.textSecondary, style = Type.caption)
                }
            }
        }
        if (openMenu != null) IconAction(Icons.More, "セッションのメニュー", openMenu, size = 20.dp)
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
            style = Type.body.copy(lineBreak = Type.phrase, textAlign = TextAlign.Center),
        )
        if (action != null) TextAction(action, onAction)
    }
}

@Composable
private fun Note(text: String, modifier: Modifier = Modifier, error: Boolean = false) {
    Text(text, modifier, if (error) pi.failedText else pi.textSecondary, style = Type.caption)
}

/** 会話の切り替えの行（Chat-Edge）。誰がどう切り替えたかと、前の会話 → いまの会話。 */
@Composable
private fun SwitchNote(
    reason: String,
    byPhone: Boolean,
    from: String,
    to: String,
    modifier: Modifier = Modifier,
) {
    val (lead, command) = switchLine(reason, byPhone)
    Column(
        modifier.fillMaxWidth().semantics(mergeDescendants = true) {},
        Arrangement.spacedBy(4.dp),
        Alignment.CenterHorizontally,
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Swap, null, Modifier.size(14.dp), pi.textSecondary)
            Text(
                buildAnnotatedString {
                    append(lead)
                    if (command != null) {
                        append(" ")
                        withStyle(SpanStyle(fontFamily = Mono)) { append(command) }
                        append(" されました")
                    }
                },
                color = pi.textSecondary,
                style = Type.caption,
            )
        }
        Text(
            buildAnnotatedString {
                if (from.isNotEmpty()) append("$from → ")
                withStyle(SpanStyle(color = pi.text, fontWeight = FontWeight.SemiBold)) {
                    append(to.ifEmpty { "新しい会話" }.lineSequence().first().take(60))
                }
            },
            color = pi.textSecondary,
            style = Type.caption.copy(textAlign = TextAlign.Center),
        )
    }
}

/** スマホでは操作できない PC の画面（Chat-Attach）。PC のターミナルへ案内し、つなぐコマンドがあればコピーできる。 */
@Composable
internal fun PcCard(title: String, command: String?) {
    Column(
        Modifier.padding(top = 24.dp)
            .fillMaxWidth()
            .background(pi.surface, RoundedCornerShape(16.dp))
            .padding(16.dp)
    ) {
        Row(
            Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
            Arrangement.spacedBy(8.dp),
            Alignment.CenterVertically,
        ) {
            Icon(Icons.Terminal, null, Modifier.size(16.dp), pi.text)
            Text(
                "PC のターミナルで操作が必要です",
                style = Type.body.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold),
            )
        }
        Text(
            (if (title.isNotEmpty()) "「$title」の画面" else "Extension の独自画面") +
                "は、スマホでは操作できません。PC で操作が終わるまで、Pi はこのまま待ちます。" +
                if (command == null) "Pi を起動した PC のターミナルで操作してください。" else "",
            Modifier.padding(top = 8.dp),
            pi.textSecondary,
            style = Type.body.copy(lineBreak = Type.phrase),
        )
        if (command != null) CommandPill(command, Modifier.padding(top = 14.dp), onSurface = true)
    }
}

@Composable
internal fun RowView(row: ChatRow, open: Boolean, toggle: () -> Unit) {
    when (row) {
        is ChatRow.User ->
            Column(Modifier.fillMaxWidth(), Arrangement.spacedBy(6.dp), Alignment.End) {
                if (row.pics.size > 0) PicGrid(row.pics)
                val accent = SpanStyle(color = pi.accentText, fontFamily = Mono, fontSize = 13.sp)
                if (row.text.isNotEmpty())
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
            Expander(row.text, open, toggle, Modifier.heightIn(min = 32.dp), wide = false)
        is ChatRow.Thinking ->
            Column {
                Expander(
                    when {
                        row.live -> "考え中…"
                        row.ms != null && row.ms >= 1000 -> "${seconds(row.ms)} 考えた"
                        else -> "考えた"
                    },
                    open,
                    toggle,
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
            }
        is ChatRow.Tool ->
            Column {
                ToolRow(row, open, toggle)
                // ツールが返した画像は、出力を畳んでいても見える。
                if (row.pics.size > 0)
                    PicGrid(row.pics, Modifier.padding(start = 22.dp, top = 4.dp, bottom = 6.dp))
            }
        is ChatRow.Note -> Note(row.text, error = row.error)
        is ChatRow.Switch -> SwitchNote(row.reason, row.byPhone, row.from, row.to)
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
    // 実行中は、始まってからの秒数を右に出す（開始時刻が分かるときだけ）。
    val running by
        produceState("", row.since) {
            while (row.since != null) {
                value = seconds((System.currentTimeMillis() - row.since).coerceAtLeast(0))
                delay(1000)
            }
        }
    val meta = if (row.since != null) running else row.meta
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
            if (row.state == ToolState.Failed) Text(meta, color = pi.failedText, style = Type.meta)
            else if (meta.startsWith("+"))
                meta.split(" ").forEach {
                    Text(
                        it,
                        color = if (it.startsWith("+")) pi.addText else pi.delText,
                        style = Type.meta,
                    )
                }
            else if (meta.isNotEmpty())
                Text(
                    meta,
                    // 秒ごとの読み上げはしない。
                    if (row.since != null) Modifier.clearAndSetSemantics {} else Modifier,
                    pi.textSecondary,
                    style = Type.meta,
                )
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
