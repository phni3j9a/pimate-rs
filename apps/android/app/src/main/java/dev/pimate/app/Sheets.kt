package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.*
import uniffi.pimate_ffi.CoreException
import uniffi.pimate_ffi.RcMethod

/** 結果を返す RC 操作の失敗を、画面の案内にする（中核は PC 側のエラー本文を渡さない）。 */
internal fun failure(e: CoreException): String =
    when (e) {
        is CoreException.Rejected -> "操作は拒否されました"
        is CoreException.Offline -> "PC に接続していません。再接続してからお試しください"
        is CoreException.Blocked -> "Pi の処理か、ほかの操作が終わるまでお待ちください"
        is CoreException.Unsupported -> "この Pi は対応していません。PC の PiMate を更新してください"
        is CoreException.StaleTarget -> "会話が変わりました。開き直してください"
        is CoreException.Timeout,
        is CoreException.OutcomeUnknown -> "結果を確認できませんでした。もう一度試す前に、PC の状態を確認してください"
        else -> "操作できませんでした"
    }

/**
 * 結果を返す RC 操作。待ち合わせは必ずメインスレッドで再開する: 画面の coroutine の文脈によっては、中核（Rust）のスレッドの上で そのまま続きを走らせてしまう（計装試験の
 * Compose の文脈がそうで、続きから中核を呼び直すと落ちる）。
 */
internal suspend fun SessionModel.ask(key: EditorKey, method: RcMethod, params: JSONObject) =
    withContext(Dispatchers.Main.immediate) { request(key, method, params) }

/**
 * シートの中の操作。対象はシートを開いた時点の EditorKey のまま（SessionModel.request が、対象が変わったあとの結果を捨てる）。
 * 結果を待つ間は重ねて送らず、失敗はシートの中で案内する。自動で再送しない。
 */
internal class Ops(
    private val m: SessionModel,
    val key: EditorKey,
    private val scope: CoroutineScope,
) {
    /** 実行中の操作の名前（行の印に使う）。空なら待っていない。 */
    var busy by mutableStateOf("")
        private set

    var error by mutableStateOf("")

    /** 開いた時点の対象が、いまも操作できる。 */
    val live
        get() = m.edits.current == key && m.target == key.target

    val ready
        get() = live && busy.isEmpty() && m.rows.groups["busy"].isNullOrEmpty()

    fun run(
        name: String,
        method: RcMethod,
        params: JSONObject = JSONObject(),
        done: (String) -> Unit = {},
    ) {
        if (busy.isNotEmpty()) return
        busy = name
        error = ""
        scope.launch {
            try {
                done(m.ask(key, method, params))
            } catch (e: CoreException) {
                error = failure(e)
            } finally {
                busy = ""
            }
        }
    }
}

@Composable
internal fun rememberOps(m: SessionModel, key: EditorKey): Ops {
    val scope = rememberCoroutineScope()
    return remember(m, key) { Ops(m, key, scope) }
}

@Composable
internal fun SheetError(text: String) {
    if (text.isNotEmpty())
        Text(
            text,
            Modifier.padding(top = 10.dp).semantics { liveRegion = LiveRegionMode.Polite },
            pi.failedText,
            style = Type.caption,
        )
}

@Composable
private fun Section(title: String, note: String = "") {
    Box(Modifier.padding(top = 28.dp, bottom = 10.dp)) { SheetTitle(title, note, Type.header) }
}

private fun JSONObject.ref() = JSONObject().put("provider", str("provider")).put("id", str("id"))

internal fun same(a: JSONObject?, b: JSONObject?) =
    a != null && b != null && a.str("provider") == b.str("provider") && a.str("id") == b.str("id")

/** モデル・Thinking（docs/design/html/Sheet-Model.html）。変更は PC の Pi にも反映される。 */
@Composable
internal fun ModelSheet(m: SessionModel, key: EditorKey, close: () -> Unit) {
    val ops = rememberOps(m, key)
    val groups = m.rows.groups
    val view = groups["view"].orEmpty().associate { it.id to it.json }
    val models = remember(groups["model"]) { groups["model"].orEmpty().map { JSONObject(it.json) } }
    var loaded by remember { mutableStateOf(models.isNotEmpty()) }
    var loadError by remember { mutableStateOf("") }
    var attempt by remember { mutableIntStateOf(0) }
    // 開くたびに読み直す（お気に入りと既定は PC 側でも変わる）。
    LaunchedEffect(key, attempt) {
        try {
            m.ask(key, RcMethod.MODELS, JSONObject())
            loaded = true
            loadError = ""
        } catch (e: CoreException) {
            loadError = failure(e)
        }
    }
    val current =
        view["model"]
            ?.takeIf { it != "null" }
            ?.let(::JSONObject)
            ?.let { ref -> models.find { same(it, ref) } }
    val thinking = view["thinking"]?.let { JSONTokener(it).nextValue() as? String }.orEmpty()
    val favorites = models.filter { it.optBoolean("favorite") }
    var tab by rememberSaveable { mutableStateOf("") }
    var saved by rememberSaveable { mutableStateOf("") }
    val scope = tab.ifEmpty { if (favorites.isEmpty()) "all" else "favorites" }
    var query by rememberSaveable { mutableStateOf("") }
    PiSheet(close) {
        SheetTitle("モデル", "PC の Pi にも反映されます")
        if (loaded) {
            Segment(
                listOf("favorites" to "お気に入り ${favorites.size}", "all" to "すべて ${models.size}"),
                scope,
                Modifier.padding(top = 14.dp),
            ) {
                tab = it
            }
            Text(
                if (saved == "favorite") "保存しました。起動中の PC の Pi の Ctrl+P には再起動後に反映されます"
                else "☆ は PC の /scoped-models（Ctrl+P で切り替えるモデル）と共通です",
                Modifier.padding(top = 6.dp),
                pi.textSecondary,
                style = Type.caption,
            )
        }
        if (models.size > 6)
            Row(
                Modifier.padding(top = 10.dp)
                    .fillMaxWidth()
                    .heightIn(min = tap)
                    .background(pi.surface, CircleShape)
                    .padding(horizontal = 14.dp),
                Arrangement.spacedBy(8.dp),
                Alignment.CenterVertically,
            ) {
                Icon(Icons.Search, null, Modifier.size(16.dp), pi.textSecondary)
                BasicTextField(
                    query,
                    { query = it },
                    Modifier.weight(1f).semantics { contentDescription = "モデルを検索" },
                    textStyle = Type.body.copy(color = pi.text),
                    singleLine = true,
                    cursorBrush = SolidColor(pi.accent),
                    decorationBox = { inner ->
                        if (query.isEmpty()) Text("モデルを検索", color = pi.textSecondary)
                        inner()
                    },
                )
            }
        SheetError(ops.error)
        val shown =
            (if (scope == "all") models else favorites).filter {
                query.isBlank() ||
                    "${it.str("provider")}/${it.str("id")} ${it.str("name")}"
                        .contains(query.trim(), true)
            }
        when {
            !loaded && loadError.isNotEmpty() ->
                Column(
                    Modifier.fillMaxWidth().padding(top = 16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(loadError, color = pi.textSecondary, style = Type.caption)
                    TextAction("再読み込み", { attempt++ })
                }
            !loaded ->
                Row(
                    Modifier.fillMaxWidth().padding(vertical = 24.dp),
                    Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                    Alignment.CenterVertically,
                ) {
                    Spinner()
                    Text("モデルを読み込み中…", color = pi.textSecondary, style = Type.caption)
                }
            shown.isEmpty() ->
                Text(
                    when {
                        query.isNotBlank() -> "一致するモデルはありません"
                        scope == "favorites" -> "お気に入りはありません。「すべて」で ☆ を付けると、ここに並びます"
                        else -> "使えるモデルがありません"
                    },
                    Modifier.padding(top = 20.dp),
                    pi.textSecondary,
                    style = Type.caption,
                )
            else ->
                Column(Modifier.selectableGroup()) {
                    shown
                        .groupBy { it.str("provider") }
                        .forEach { (provider, list) ->
                            Text(
                                providerName(provider),
                                Modifier.padding(start = 4.dp, top = 16.dp, bottom = 2.dp)
                                    .semantics { heading() },
                                pi.textSecondary,
                                style = Type.caption,
                                fontWeight = FontWeight.SemiBold,
                            )
                            list.forEach { model ->
                                ModelRow(
                                    model,
                                    model === current,
                                    ops.ready,
                                    select = { ops.run("model", RcMethod.SET_MODEL, model.ref()) },
                                ) {
                                    val next =
                                        if (model.optBoolean("favorite"))
                                            favorites.filterNot { it === model }
                                        else favorites + model
                                    ops.run(
                                        "favorite",
                                        RcMethod.SET_SCOPED_MODELS,
                                        JSONObject().put("models", JSONArray(next.map { it.ref() })),
                                    ) {
                                        saved = "favorite"
                                    }
                                }
                            }
                        }
                }
        }

        Section("Thinking", "このモデルで使えるレベル")
        val levels = current?.optJSONArray("thinkingLevels")
        if (levels != null && levels.length() > 1)
            Segment(
                List(levels.length()) { levels.optString(it) }.map { it to thinkingShort(it) },
                thinking,
                Modifier.semantics { contentDescription = "Thinking のレベル" },
                ops.ready,
            ) {
                ops.run("thinking", RcMethod.SET_THINKING, JSONObject().put("level", it))
            }
        else
            Text(
                if (current != null) "このモデルは Thinking に対応していません" else "モデルの情報を読み込むと選べます",
                color = pi.textSecondary,
                style = Type.caption,
            )

        // 旧 RC は既定を返さない（不明）。そのときは区画ごと出さない。
        val default = view["defaultModel"]?.takeIf { it != "null" }?.let(::JSONObject)
        if (default != null) {
            Section("新しい Pi の既定", "PC の /model の既定と共通です")
            val ref = default.optJSONObject("model")
            val level = default.str("thinking")
            // Thinking は、段階のあるモデルのときだけ一緒に保存する。
            val toSave =
                thinking.takeIf { levels != null && levels.length() > 1 && it.isNotEmpty() }
            val isDefault = same(current, ref) && (toSave == null || toSave == level)
            Text(
                ref?.let { r ->
                    val known = models.find { same(it, r) }
                    modelLabel(
                        r.str("id"),
                        known?.str("name"),
                        level,
                        known?.optJSONArray("thinkingLevels")?.length(),
                    )
                } ?: "未設定（Pi が自動で選びます）",
                style = Type.lead,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            TextAction(
                if (isDefault) "現在のモデルと Thinking は既定です" else "現在のモデルと Thinking を既定にする",
                {
                    current?.let {
                        ops.run(
                            "default",
                            RcMethod.SET_DEFAULT_MODEL,
                            JSONObject().put("model", it.ref()).apply {
                                if (toSave != null) put("thinking", toSave)
                            },
                        ) {
                            saved = "default"
                        }
                    }
                },
                Modifier.offset(x = (-14).dp),
                enabled = ops.ready && current != null && !isDefault,
            )
            if (saved == "default")
                Text(
                    "保存しました。以後 PC とアプリで起動する新しい Pi に使われます",
                    color = pi.textSecondary,
                    style = Type.caption,
                )
        }
    }
}

@Composable
private fun ModelRow(
    model: JSONObject,
    selected: Boolean,
    enabled: Boolean,
    select: () -> Unit,
    star: () -> Unit,
) {
    val favorite = model.optBoolean("favorite")
    val name = model.str("name")
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = 56.dp)
            .clip(RoundedCornerShape(12.dp))
            .selectable(selected, enabled && !selected, Role.RadioButton, select)
            .padding(start = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(vertical = 6.dp)) {
            Text(
                name,
                style = Type.lead,
                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                "${model.str("provider")}/${model.str("id")}" +
                    if (model.has("contextWindow"))
                        " · ${count(model.optDouble("contextWindow")).uppercase()}"
                    else "",
                color = pi.textSecondary,
                style = Type.meta.copy(fontFamily = Mono),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (selected) Icon(Icons.Check, "選択中", Modifier.size(18.dp), pi.accent)
        IconAction(
            if (favorite) Icons.StarOn else Icons.Star,
            if (favorite) "$name をお気に入りから外す" else "$name をお気に入りに追加",
            star,
            enabled = enabled,
            tint = if (favorite) pi.accent else pi.textTertiary,
            size = 20.dp,
        )
    }
}

/** 使用状況（docs/design/html/Sheet-Usage.html）。この Pi のコンテキストと、同じ PC で見えた最新の利用枠。 */
@Composable
internal fun UsageSheet(m: SessionModel, key: EditorKey, close: () -> Unit) {
    val ops = rememberOps(m, key)
    val groups = m.rows.groups
    val context =
        groups["view"]
            .orEmpty()
            .find { it.id == "context" }
            ?.json
            ?.takeIf { it != "null" }
            ?.let(::JSONObject)
    // 「あと◯分」を進める。
    val now by
        produceState(System.currentTimeMillis()) {
            while (true) {
                delay(30_000)
                value = System.currentTimeMillis()
            }
        }
    // 履歴・利用枠が変わるたびに、中核の集計を読み直す。
    val stats = remember(groups["entry"]) { m.stats() }
    val allowances = remember(groups["quota"], now) { m.allowances(now.toULong()) }
    val compacting =
        ops.busy == "compact" ||
            groups["busy"].orEmpty().any { JSONObject(it.json).str("method") == "compact" }
    PiSheet(close) {
        SheetTitle("使用状況", "利用枠は同じ PC の最新値")
        Row(
            Modifier.padding(top = 20.dp, bottom = 10.dp),
            Arrangement.spacedBy(6.dp),
            Alignment.CenterVertically,
        ) {
            Icon(Icons.Context, null, Modifier.size(16.dp), pi.text)
            Text("コンテキスト", Modifier.weight(1f).semantics { heading() }, style = Type.header)
            Text(
                contextLine(context) ?: "不明",
                color = pi.textSecondary,
                style = Type.body.copy(fontSize = 13.sp),
            )
        }
        val percent = context?.takeUnless { it.isNull("percent") }?.optDouble("percent")
        UsageBar(percent, "コンテキスト")
        Row(Modifier.heightIn(min = tap), verticalAlignment = Alignment.CenterVertically) {
            Text(
                "このセッション · ${cost(stats?.cost ?: 0.0)}" +
                    if ((stats?.tokens ?: 0.0) > 0) " · ${count(stats!!.tokens)} トークン" else "",
                Modifier.weight(1f),
                pi.textSecondary,
                style = Type.caption,
            )
            if (compacting) {
                Spinner()
                Text("圧縮中…", Modifier.padding(start = 6.dp), pi.textSecondary, style = Type.caption)
            } else
                TextAction(
                    "圧縮する",
                    { ops.run("compact", RcMethod.COMPACT) },
                    Modifier.offset(x = 14.dp),
                    enabled = ops.ready,
                )
        }
        SheetError(ops.error)
        allowances.forEach { a ->
            val name = if (a.source == "claude") "Claude" else "Codex"
            Row(
                Modifier.padding(top = 20.dp, bottom = 6.dp),
                Arrangement.spacedBy(6.dp),
                Alignment.CenterVertically,
            ) {
                if (a.source == "claude")
                    Icon(Icons.Claude, null, Modifier.size(16.dp), Color(0xFFD97757))
                else Icon(Icons.OpenAI, null, Modifier.size(16.dp), pi.text)
                Text(name, Modifier.weight(1f).semantics { heading() }, style = Type.header)
                a.observedAt?.let {
                    Text(
                        "${clockTime(it.toLong(), now)} 取得",
                        color = pi.textSecondary,
                        style = Type.caption,
                    )
                }
            }
            // 利用枠を出さない旧 RC と、まだ一度も見えていない場合は区別できない（どちらも不明）。
            if (a.windows.isEmpty())
                Text(
                    "不明（この PC の Pi で $name のサブスクを使うと表示されます）",
                    color = pi.textSecondary,
                    style = Type.caption,
                )
            a.windows.forEach { w ->
                val label = windowLabel(w.minutes.toLong())
                val reset = resetLabel(w.resetsAt?.toLong(), now)
                Row(Modifier.padding(top = 8.dp, bottom = 8.dp)) {
                    Text(
                        "$label · " + (w.usedPercent?.let { "${Math.round(it)}% 使用" } ?: "—"),
                        Modifier.padding(end = 12.dp),
                        pi.textSecondary,
                        style = Type.body.copy(fontSize = 13.sp),
                    )
                    Text(
                        reset,
                        Modifier.weight(1f),
                        pi.textSecondary,
                        // 折り返すときは「（あと…）」の前で分ける。
                        style =
                            Type.caption.copy(
                                textAlign = androidx.compose.ui.text.style.TextAlign.End,
                                lineBreak = Type.phrase,
                            ),
                    )
                }
                UsageBar(w.usedPercent, "$name $label の利用枠、$reset")
            }
        }
        Text(
            "この PC の Pi（バックグラウンドのサブエージェントを含む）の最新の応答から取得します。他の端末での利用は、次の応答まで反映されません",
            Modifier.padding(top = 20.dp),
            pi.textTertiary,
            style = Type.caption,
        )
    }
}

private fun agoLabel(at: Long, now: Long) =
    ago(at, now).let { if (it != "昨日" && it.last() in "分間日") "${it}前" else it }

/**
 * セッション・Instance（docs/design/html/Sheet-Session.html）。page: 最初に出す面（session / sessions / fork）。
 * 受け付けられた操作はシートを閉じ、結果は会話の欄で示す。拒否はシートの中に残す。
 */
@Composable
internal fun SessionSheet(
    m: SessionModel,
    key: EditorKey,
    start: String,
    open: (String) -> Unit,
    close: () -> Unit,
) {
    val ops = rememberOps(m, key)
    var page by rememberSaveable { mutableStateOf(start) }
    val groups = m.rows.groups
    val view = groups["view"].orEmpty().associate { it.id to it.json }
    PiSheet(close) {
        when (page) {
            "sessions" ->
                SessionsPage(m, ops, if (start == page) null else ({ page = start }), close)
            "fork" -> ForkPage(m, ops, if (start == page) null else ({ page = start }), close)
            else -> {
                val session = view["session"]?.takeIf { it != "null" }?.let(::JSONObject)
                val cwd = view["cwd"]?.let { JSONTokener(it).nextValue() as? String }.orEmpty()
                val stats = remember(groups["entry"]) { m.stats() }
                Text(
                    title(session, cwd),
                    Modifier.semantics { heading() },
                    style = Type.sheet,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    listOfNotNull(tilde(cwd).ifEmpty { null }, m.host?.name).joinToString(" · "),
                    Modifier.padding(top = 2.dp),
                    pi.textSecondary,
                    style = Type.meta.copy(fontFamily = Mono),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                val strong = SpanStyle(color = pi.text, fontWeight = FontWeight.SemiBold)
                Text(
                    buildAnnotatedString {
                        append("メッセージ ")
                        withStyle(strong) { append("${stats?.messages ?: 0u}") }
                        append(" · トークン ")
                        withStyle(strong) { append(count(stats?.tokens ?: 0.0).uppercase()) }
                        append(" · コスト ")
                        withStyle(strong) { append(cost(stats?.cost ?: 0.0)) }
                    },
                    Modifier.padding(top = 12.dp),
                    pi.textSecondary,
                    style = Type.body.copy(fontSize = 13.sp),
                )
                SheetError(ops.error)

                Label("会話")
                val users =
                    remember(groups["entry"]) {
                        groups["entry"].orEmpty().any { JSONObject(it.json).str("kind") == "user" }
                    }
                MenuRow("新しいセッション", "/new", ops.ready, ops.busy == "new") {
                    ops.run("new", RcMethod.SESSION_OP, JSONObject().put("op", "new")) { close() }
                }
                MenuRow("過去のセッションを開く", "/resume", ops.ready, next = true) { page = "sessions" }
                // 旧 RC にはツリーがない。押しても開けない入口は出さない。
                if (view["sessionTree"] == "true")
                    MenuRow("セッションツリー", "/tree", ops.ready, next = true, onClick = { open("tree") })
                MenuRow("ここから分岐", "/fork", ops.ready && users, next = true) { page = "fork" }
                MenuRow("コンテキストを圧縮", "/compact", ops.ready, ops.busy == "compact") {
                    ops.run("compact", RcMethod.COMPACT) { close() }
                }

                MenuRow("変更レビュー", "", ops.ready, next = true) { open("review") }
                MenuRow("GitHub の Issue / PR", "", ops.ready, next = true) { open("github") }
                if (!groups["run"].isNullOrEmpty())
                    MenuRow("サブエージェント", "", ops.ready, next = true) { open("agents") }

                Label("この Pi")
                val origin =
                    when (view["origin"]) {
                        "\"grok-bot\"" -> "Grok Bot"
                        "\"mobile\"" -> "スマホ"
                        "\"pc\"" -> "PC"
                        else -> null
                    }
                // PC のターミナルがつながっているかは一覧の行にある。旧 Host が出さなければ書かない。
                val attached =
                    groups["instance"]
                        .orEmpty()
                        .find { it.id == m.selected }
                        ?.let { JSONObject(it.json).opt("pcAttached") as? Boolean }
                        ?.let { if (it) "PC から接続中" else "PC から未接続" }
                if (origin != null)
                    Row(
                        Modifier.heightIn(min = tap),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("起動元", Modifier.weight(1f), style = Type.lead)
                        Text(
                            listOfNotNull(origin, attached).joinToString(" · "),
                            color = pi.textSecondary,
                            style = Type.body.copy(fontSize = 13.sp),
                        )
                    }
                val command = m.attachCommand()
                if (command != null) CommandPill(command, Modifier.padding(top = 6.dp))
                Text(
                    if (command != null) "PC のターミナルで実行すると、同じ Pi の画面に接続できます。"
                    else "この Pi は PC のターミナルで直接動いています。",
                    Modifier.padding(top = 8.dp),
                    pi.textSecondary,
                    style = Type.caption,
                )
                ConfirmAction(
                    key,
                    "終了する",
                    "作業中の処理は止まります。会話はセッションに保存され、あとで開き直せます。",
                    title = "この Pi を終了しますか？",
                    button = if (ops.busy == "shutdown") "終了しています…" else "この Pi を終了…",
                    enabled = ops.ready,
                    modifier = Modifier.padding(top = 10.dp).offset(x = (-14).dp),
                ) {
                    if (ops.ready) ops.run("shutdown", RcMethod.SHUTDOWN) { close() }
                }
            }
        }
    }
}

@Composable
internal fun Label(text: String) {
    Text(
        text,
        Modifier.padding(top = 28.dp, bottom = 2.dp).semantics { heading() },
        pi.textSecondary,
        style = Type.caption,
        fontWeight = FontWeight.SemiBold,
    )
}

/** メニューの行（52dp）。右は、同じ操作の PC のコマンド名。 */
@Composable
internal fun MenuRow(
    label: String,
    command: String,
    enabled: Boolean,
    busy: Boolean = false,
    next: Boolean = false,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = 52.dp)
            .clip(RoundedCornerShape(12.dp))
            .clickable(enabled, role = Role.Button, onClick = onClick)
            .alpha(if (enabled || busy) 1f else .4f),
        Arrangement.spacedBy(10.dp),
        Alignment.CenterVertically,
    ) {
        Text(label, Modifier.weight(1f), style = Type.lead)
        if (busy) Spinner()
        else
            Text(
                command,
                // コマンド名は補足。読み上げは操作の名前だけにする。
                Modifier.clearAndSetSemantics {},
                pi.textSecondary,
                style = Type.meta.copy(fontFamily = Mono),
            )
        if (next) Icon(Icons.Right, null, Modifier.size(16.dp), pi.textTertiary)
    }
}

@Composable
private fun PageHeader(title: String, back: (() -> Unit)?) {
    Row(Modifier.heightIn(min = tap), verticalAlignment = Alignment.CenterVertically) {
        if (back != null) IconAction(Icons.Back, "戻る", back, Modifier.offset(x = (-12).dp))
        Text(
            title,
            Modifier.weight(1f).offset(x = if (back != null) (-10).dp else 0.dp).semantics {
                heading()
            },
            style = Type.sheet,
        )
    }
}

@Composable
private fun ListRow(
    title: String,
    meta: String,
    enabled: Boolean,
    current: Boolean = false,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = 56.dp)
            .clip(RoundedCornerShape(12.dp))
            .selectable(current, enabled && !current, Role.Button, onClick)
            .padding(vertical = 8.dp),
        Arrangement.spacedBy(10.dp),
        Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = Type.lead, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (meta.isNotEmpty())
                Text(
                    if (current) "$meta · 開いています" else meta,
                    Modifier.padding(top = 2.dp),
                    pi.textSecondary,
                    style = Type.caption,
                )
        }
        if (current) Icon(Icons.Check, null, Modifier.size(18.dp), pi.accent)
    }
}

/** 過去のセッション（/resume）。開いた時点の一覧を出し、選ぶと切り替える。 */
@Composable
private fun SessionsPage(m: SessionModel, ops: Ops, back: (() -> Unit)?, close: () -> Unit) {
    var list by remember { mutableStateOf<JSONArray?>(null) }
    var error by remember { mutableStateOf("") }
    val now = remember { System.currentTimeMillis() }
    LaunchedEffect(ops.key) {
        try {
            list =
                JSONObject(m.ask(ops.key, RcMethod.SESSIONS, JSONObject())).optJSONArray("sessions")
                    ?: JSONArray()
        } catch (e: CoreException) {
            error = failure(e)
        }
    }
    PageHeader("過去のセッション", back)
    SheetError(ops.error)
    val sessions = list
    when {
        error.isNotEmpty() ->
            Text(error, Modifier.padding(vertical = 24.dp), pi.textSecondary, style = Type.caption)
        sessions == null ->
            Row(
                Modifier.fillMaxWidth().padding(vertical = 28.dp),
                Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                Alignment.CenterVertically,
            ) {
                Spinner()
                Text("読み込み中…", color = pi.textSecondary, style = Type.caption)
            }
        sessions.length() == 0 ->
            Text(
                "このフォルダに保存されたセッションはありません",
                Modifier.padding(vertical = 24.dp),
                pi.textSecondary,
                style = Type.caption,
            )
        else ->
            for (i in 0 until sessions.length()) {
                val s = sessions.optJSONObject(i) ?: continue
                ListRow(
                    s.str("name").ifEmpty { s.str("firstMessage") }.ifEmpty { "（空のセッション）" },
                    "${agoLabel(s.optLong("modified"), now)} · ${s.optInt("messageCount")} 件",
                    ops.ready,
                    s.optBoolean("current"),
                ) {
                    ops.run(
                        "switch",
                        RcMethod.SESSION_OP,
                        JSONObject().put("op", "switch").put("arg", s.str("path")),
                    ) {
                        close()
                    }
                }
            }
    }
}

/** 分岐（/fork）の元にするメッセージを選ぶ。 */
@Composable
private fun ForkPage(m: SessionModel, ops: Ops, back: (() -> Unit)?, close: () -> Unit) {
    val entries = m.rows.groups["entry"]
    val users =
        remember(entries) {
            entries
                .orEmpty()
                .map { JSONObject(it.json) }
                .filter { it.str("kind") == "user" }
                .asReversed()
        }
    val now = remember { System.currentTimeMillis() }
    PageHeader("どのメッセージから分岐しますか", back)
    Text(
        "選んだメッセージの直前までを新しいセッションに写します。メッセージは入力欄に入るので、直してから送れます。",
        color = pi.textSecondary,
        style = Type.caption,
    )
    SheetError(ops.error)
    if (users.isEmpty())
        Text(
            "分岐できるメッセージがありません",
            Modifier.padding(vertical = 24.dp),
            pi.textSecondary,
            style = Type.caption,
        )
    users.forEach { u ->
        ListRow(
            u.str("text").ifEmpty { "（画像のみ）" },
            if (!u.isNull("timestamp")) agoLabel(u.optLong("timestamp"), now) else "",
            ops.ready,
        ) {
            ops.run(
                "fork",
                RcMethod.SESSION_OP,
                JSONObject().put("op", "fork").put("arg", u.str("entryId")),
            ) {
                close()
            }
        }
    }
}
