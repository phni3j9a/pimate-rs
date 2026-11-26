package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.*
import uniffi.pimate_ffi.CoreException
import uniffi.pimate_ffi.Operation

@Composable
internal fun ReviewPage(
    m: SessionModel,
    at: EditorKey,
    close: () -> Unit,
    initialTurn: String? = null,
) {
    val items = m.rows.groups["entry"].orEmpty()
    val cwd =
        m.rows.groups["instance"]
            .orEmpty()
            .find { it.id == at.target.instance }
            ?.let { JSONObject(it.json).str("cwd") }
            .orEmpty()
    var all by rememberSaveable { mutableStateOf(false) }
    var turn by rememberSaveable { mutableStateOf(initialTurn) }
    var choose by remember { mutableStateOf(false) }
    var data by remember(items, cwd, all, turn) { mutableStateOf<Review?>(null) }
    var turns by remember { mutableStateOf(emptyList<Pair<String, String>>()) }
    var error by remember { mutableStateOf("") }
    LaunchedEffect(items, cwd, all, turn) {
        val (r, users) =
            withContext(Dispatchers.Default) {
                val entries = items.map { JSONObject(it.json) }
                val full = review(entries, cwd)
                val id = turn ?: full.latestChangedTurn
                (if (all || id == null) full else review(entries, cwd, id)) to
                    buildList {
                        if (entries.isNotEmpty() && entries.first().str("kind") != "user")
                            add(BEGINNING_TURN to "最初の入力より前")
                        entries
                            .filter { it.str("kind") == "user" }
                            .forEach { add(it.str("entryId") to it.str("text").take(80)) }
                    }
            }
        data = r
        turns = users
    }
    Page("変更", close) {
        Hint("${data?.files?.size ?: "…"} ファイル · 読み込み済みの履歴")
        Segment(listOf("turn" to "このターン", "all" to "セッション全体"), if (all) "all" else "turn") {
            all = it == "all"
            choose = false
        }
        if (!all && turns.size > 1) {
            TextAction("ターンを選ぶ", { choose = !choose })
            if (choose)
                turns.asReversed().forEach { (id, name) ->
                    MenuRow(name, if (data?.turn == id) "選択中" else "", true) {
                        turn = id
                        choose = false
                    }
                }
        }
        data?.let { r ->
            val patch = remember(r) { clipboardPatch(r.files) }
            if (patch != "") CopyAction(patch, "差分をコピー")
            if (r.files.isEmpty()) Hint("変更はありません")
            r.files.forEachIndexed { index, file ->
                key(file.path) {
                    var expanded by rememberSaveable { mutableStateOf(index == 0) }
                    val diffs = file.pieces.mapNotNull { it.diff }
                    MenuRow(
                        (if (file.pieces.first().created) "A　" else "M　") + file.path,
                        if (diffs.isEmpty()) ""
                        else "+${diffs.sumOf { it.add }} −${diffs.sumOf { it.del }}",
                        true,
                    ) {
                        expanded = !expanded
                    }
                    if (expanded) file.pieces.forEach { piece -> ChangeDiff(piece) }
                }
            }
            if (r.files.isNotEmpty())
                PrimaryButton(
                    "この変更について依頼",
                    {
                        try {
                            m.askAbout(at, r)
                            close()
                        } catch (e: uniffi.pimate_ffi.CoreException) {
                            error = failure(e)
                        }
                    },
                    Modifier.fillMaxWidth(),
                )
        } ?: Hint("変更を集計しています…")
        SheetError(error)
    }
}

/**
 * Empty if there are no patches, null if the complete selection would exceed the clipboard budget.
 */
internal fun clipboardPatch(files: List<ReviewFile>): String? = buildString {
    for (file in files) for (piece in file.pieces) {
        val patch = piece.patch?.takeIf { it.isNotEmpty() } ?: continue
        val newline = !patch.endsWith('\n')
        if (patch.length > CLIPBOARD_CHARS - length - (if (newline) 1 else 0)) return null
        append(patch)
        if (newline) append('\n')
    }
}

@Composable
internal fun ChangeDiff(piece: Piece, onExpand: () -> Unit = {}) {
    SelectionContainer {
        Column(
            Modifier.fillMaxWidth()
                .background(pi.surface, RoundedCornerShape(12.dp))
                .horizontalScroll(rememberScrollState())
                .padding(10.dp)
        ) {
            if (piece.diff?.hunks?.isNotEmpty() == true)
                piece.diff.hunks.forEach { h ->
                    Text(h.header, style = Type.mono, color = pi.textSecondary)
                    var limit by remember(h) { mutableIntStateOf(120) }
                    h.lines.take(limit).forEach { line ->
                        val add = line.old == null
                        val del = line.new == null
                        Text(
                            "${line.old ?: " "}\t${line.new ?: " "}  ${if(add) "+" else if(del) "−" else " "} ${line.text}",
                            style = Type.mono,
                            color = if (add) pi.addText else if (del) pi.delText else pi.text,
                        )
                    }
                    if (h.lines.size > limit)
                        TextAction(
                            "続きを表示（残り ${h.lines.size-limit} 行）",
                            {
                                onExpand()
                                limit += 120
                            },
                        )
                }
            else {
                Hint(
                    when (piece.omitted) {
                        "binary" -> "バイナリファイル"
                        "too_large" -> "大きいため差分を省略しました"
                        "unknown_before" -> "変更前の内容は不明です"
                        "concurrent" -> "同時に変更されたため差分は不明です"
                        else -> "差分情報はありません"
                    }
                )
                (piece.content ?: piece.patch)?.let { Text(it, style = Type.mono) }
            }
        }
    }
}

internal fun webUrl(url: String): String? =
    try {
        java.net
            .URI(url)
            .takeIf {
                it.scheme?.lowercase() in setOf("http", "https") &&
                    !it.host.isNullOrEmpty() &&
                    it.userInfo == null
            }
            ?.toString()
    } catch (_: java.net.URISyntaxException) {
        null
    }

internal fun ChatEdits.insertGithub(
    at: EditorKey,
    target: uniffi.pimate_ffi.Target?,
    item: JSONObject,
): Boolean {
    if (at != current || at.target != target) return false
    val url = webUrl(item.str("url"))
    val ref = "#${item.getLong("number")}「${item.str("title")}」" + (url?.let { " $it" } ?: "")
    text(at, if (body.text.isEmpty()) ref else "${body.text}\n$ref")
    return true
}

internal fun githubMetadata(item: JSONObject, pr: Boolean): String =
    buildList {
            add(item.str("state"))
            if (pr) {
                when {
                    item.optBoolean("isDraft") -> add("ドラフト")
                    else ->
                        when (val decision = item.str("reviewDecision")) {
                            "APPROVED" -> add("承認済み")
                            "CHANGES_REQUESTED" -> add("変更の依頼あり")
                            "REVIEW_REQUIRED" -> add("レビュー待ち")
                            "" -> Unit
                            else -> add(decision)
                        }
                }
                item
                    .optJSONObject("ci")
                    ?.takeIf { ci ->
                        listOf("passed", "failed", "pending").all { ci.opt(it) is Number }
                    }
                    ?.let {
                        add(
                            "CI 成功 ${it.getInt("passed")}・失敗 ${it.getInt("failed")}・待機 ${it.getInt("pending")}"
                        )
                    }
            }
            addAll(item.optJSONArray("labels").strings())
        }
        .filter { it.isNotEmpty() }
        .joinToString(" · ")

internal fun githubRejected(status: JSONObject?) =
    when {
        status?.opt("installed") == false -> "PC に GitHub CLI（gh）をインストールし、gh auth login を実行してください。"
        status?.opt("loggedIn") == false -> "PC で gh auth login を実行してください。"
        status?.opt("loggedIn") == true ->
            "作業中の Pi が GitHub のリポジトリで動いているか、対象の Issue / PR と閲覧権限を確認してください。"
        else -> "取得できませんでした。PC の gh の状態を確認してください。"
    }

@Composable
internal fun GithubPage(m: SessionModel, at: EditorKey, close: () -> Unit) {
    var tab by rememberSaveable { mutableStateOf("issue") }
    var number by rememberSaveable { mutableIntStateOf(0) }
    var retry by remember { mutableIntStateOf(0) }
    var data by remember(number, retry) { mutableStateOf<JSONObject?>(null) }
    var error by remember(number, retry) { mutableStateOf("") }
    LaunchedEffect(at, number, retry) {
        checked({ error = it }) {
            val q = JSONObject().put("kind", if (number == 0) "list" else tab)
            if (number != 0) q.put("number", number)
            try {
                data = m.github(at, q).getJSONObject("data")
            } catch (_: CoreException.Rejected) {
                var status: JSONObject? = null
                if (m.rowsFor(at.host).info().optBoolean("githubStatus"))
                    checked({}) {
                        status =
                            JSONObject(m.machineRequest(at.host, Operation.GithubStatus))
                                .optJSONObject("status")
                    }
                error = githubRejected(status)
            }
        }
    }
    Page(
        if (number == 0) "GitHub" else "${if(tab=="issue") "Issue" else "PR"} #$number",
        { if (number == 0) close() else number = 0 },
    ) {
        if (error.isNotEmpty()) {
            SheetError(error)
            TextAction("再読み込み", { retry++ })
        } else if (data == null) Hint("PC から読み込み中…")
        data?.let { r ->
            if (number == 0) {
                Text(r.optJSONObject("repo")?.str("name").orEmpty(), style = Type.sheet)
                Segment(listOf("issue" to "Issue", "pr" to "PR"), tab) { tab = it }
                val list = r.optJSONArray(if (tab == "issue") "issues" else "prs").objects()
                if (list.isEmpty()) Hint("該当する項目はありません")
                list.forEach { v ->
                    ListRow(
                        "#${v.optInt("number")}　${v.str("title")}",
                        githubMetadata(v, tab == "pr"),
                        true,
                    ) {
                        number = v.getInt("number")
                    }
                }
            } else
                r.optJSONObject("item")?.let { v ->
                    CompositionLocalProvider(LocalPics provides Pics.Local) {
                        Text(v.str("title"), style = Type.title)
                        Hint("${v.str("author")} · ${githubMetadata(v, tab == "pr")}")
                        if (tab == "pr") {
                            if (
                                v.str("headRefName").isNotEmpty() &&
                                    v.str("baseRefName").isNotEmpty()
                            )
                                Hint("${v.str("headRefName")} → ${v.str("baseRefName")}")
                            if (
                                listOf("changedFiles", "additions", "deletions").all {
                                    v.opt(it) is Number
                                }
                            )
                                Hint(
                                    "変更 ${v.optInt("changedFiles")} ファイル · +${v.optInt("additions")} / −${v.optInt("deletions")}"
                                )
                            Label("CI チェック")
                            if (
                                !v.optBoolean("checksTruncated") &&
                                    v.optJSONArray("checks").objects().isEmpty()
                            )
                                Hint(if (v.has("checks")) "チェックはありません" else "CI チェックの内訳は未提供です")
                        }
                        v.optJSONArray("checks").objects().forEach {
                            Hint("${it.str("name")} · ${it.str("state")}")
                        }
                        if (v.optBoolean("checksTruncated"))
                            Hint("CI チェックの一部を省略しています。続きは GitHub で確認してください。")
                        Markdown(v.str("body").ifEmpty { "本文はありません" })
                        if (v.optBoolean("bodyTruncated")) Hint("本文の一部を省略しています")
                        Label("コメント")
                        if (v.optJSONArray("comments").objects().isEmpty()) Hint("コメントはありません")
                        v.optJSONArray("comments").objects().forEach {
                            Text(it.str("author"), style = Type.header)
                            Markdown(it.str("body"))
                            if (it.optBoolean("bodyTruncated")) Hint("一部を省略しています")
                        }
                        if (v.optBoolean("commentsTruncated")) Hint("最新のコメントのみ表示しています")
                        webUrl(v.str("url"))?.let { External("GitHub で開く", it) }
                        PrimaryButton(
                            "Pi に依頼",
                            {
                                if (m.edits.insertGithub(at, m.target, v)) close()
                                else error = "Pi が切り替わりました。下書きは変更していません。"
                            },
                            Modifier.fillMaxWidth(),
                        )
                    }
                }
        }
    }
}

internal fun agentState(state: String) =
    when (state) {
        "running" -> "実行中"
        "waiting" -> "入力待ち"
        "stopping" -> "停止中"
        "paused" -> "一時停止"
        "complete" -> "完了"
        "failed" -> "失敗"
        "queued" -> "待機中"
        "stopped" -> "停止済み"
        else -> "状態不明"
    }

private fun agentAction(id: String) =
    when (id) {
        "steer" -> "割り込む"
        "follow_up" -> "完了後に指示"
        "resume" -> "再開"
        "abort_parent" -> "親の Pi ごと停止"
        "stop_child" -> "この子を停止"
        "interrupt" -> "実行全体を一時停止"
        else -> "実行全体を停止"
    }

internal fun resumedChild(runs: List<JSONObject>, next: String?, index: Int): String? {
    if (next == null) return null
    val children = runs.find { it.str("id") == next }?.optJSONArray("children").objects()
    return (children.find { it.optInt("index", -1) == index } ?: children.firstOrNull())?.str("id")
}

@Composable
internal fun AgentsPage(
    m: SessionModel,
    at: EditorKey,
    close: () -> Unit,
    initial: String? = null,
) {
    var rid by rememberSaveable { mutableStateOf(initial.orEmpty()) }
    val rows = m.rows.groups["run"].orEmpty()
    val runs = remember(rows) { rows.map { JSONObject(it.json) } }
    val selectedRun = runs.find { it.str("id") == initial }
    var cid by rememberSaveable {
        mutableStateOf(
            selectedRun?.optJSONArray("children").objects().singleOrNull()?.str("id").orEmpty()
        )
    }
    if (cid.isEmpty())
        PiSheet(close) {
            SheetTitle("サブエージェント")
            for (open in listOf(true, false)) {
                val group =
                    runs.filter {
                        it.optBoolean("open") == open &&
                            (initial == null || it.str("id") == initial)
                    }
                if (group.isNotEmpty()) Label(if (open) "実行中・再開可能" else "以前の実行")
                group.forEach { run ->
                    key(run.str("id")) {
                        Hint("${run.str("backend")} · ${run.str("mode")}")
                        run.optJSONArray("children").objects().forEach { c ->
                            val detail =
                                listOfNotNull(
                                        c.str("agent"),
                                        c.optLong("startedAt")
                                            .takeIf { it > 0 }
                                            ?.let {
                                                elapsed(
                                                    (c.optLong(
                                                            "endedAt",
                                                            System.currentTimeMillis(),
                                                        ) - it)
                                                        .coerceAtLeast(0)
                                                )
                                            },
                                        c.optJSONObject("activity")?.let {
                                            it.str("tool") + " " + it.str("args")
                                        },
                                    )
                                    .joinToString(" · ")
                            AgentLink(
                                c.str("title").ifEmpty { c.str("agent") },
                                detail,
                                agentState(c.str("state")),
                            ) {
                                rid = run.str("id")
                                cid = c.str("id")
                            }
                        }
                        StopAgent(m, at, run, null)
                    }
                }
            }
            if (runs.isEmpty()) Hint("実行の記録はありません")
        }
    else
        key(rid, cid) {
            val run = runs.find { it.str("id") == rid }
            val child = run?.optJSONArray("children").objects().find { it.str("id") == cid }
            var resumedTo by rememberSaveable { mutableStateOf<String?>(null) }
            var resumedIndex by rememberSaveable { mutableIntStateOf(-1) }
            val resolved = resumedChild(runs, resumedTo, resumedIndex)
            LaunchedEffect(resumedTo, resolved) {
                val next = resumedTo
                if (next != null && resolved != null) {
                    rid = next
                    cid = resolved
                }
            }
            Page(
                child?.str("title").orEmpty().ifEmpty { "サブエージェント" },
                {
                    if (initial != null && selectedRun?.optJSONArray("children")?.length() == 1)
                        close()
                    else cid = ""
                },
            ) {
                if (resumedTo != null) {
                    Hint("再開先の作業記録を待っています…")
                    return@Page
                }
                if (child == null || run == null) {
                    Hint("この実行は一覧にありません")
                    return@Page
                }
                Hint(
                    listOf(child.str("agent"), child.str("model"), agentState(child.str("state")))
                        .filter(String::isNotEmpty)
                        .joinToString(" · ")
                )
                run.optJSONObject("stage")?.let {
                    Hint("チェーン ${it.optInt("index")+1} / ${it.optInt("count")}")
                }
                val record = remember { m.workRecord(at, rid, cid) }
                var retry by remember { mutableIntStateOf(0) }
                var error by remember { mutableStateOf("") }
                var loading by remember { mutableStateOf(false) }
                val scope = rememberCoroutineScope()
                val has = child.optBoolean("hasTranscript")
                LaunchedEffect(run.toString(), retry) {
                    if (has) {
                        loading = true
                        try {
                            checked({ error = it }) {
                                error = ""
                                while (record.load()) delay(150)
                            }
                        } finally {
                            loading = false
                        }
                    }
                }
                Label("親の Pi からの依頼")
                Text(record.task.ifEmpty { if (loading) "読み込み中…" else "依頼文は取得されていません" })
                Label("子の作業記録")
                val sent =
                    m.instructions.forChild(
                        ChildKey(at, rid, cid),
                        run.str("resumedFrom").ifEmpty { null },
                    )
                val transcript =
                    remember(record.items) {
                        buildRows(record.items.map(::entry), emptyList(), false).filterNot {
                            it is ChatRow.Summary
                        }
                    }
                val captions =
                    remember(sent, transcript) {
                        val unused = sent.toMutableList()
                        transcript.filterIsInstance<ChatRow.User>().associate { row ->
                            val i = unused.indexOfFirst { it.text.trim() == row.text.trim() }
                            row.key to
                                (if (i < 0) "追加の指示"
                                else "あなた · ${agentAction(unused.removeAt(i).action)}")
                        }
                    }
                // 子の entry ID やファイルを親の Pi への画像要求に流用しない。
                CompositionLocalProvider(LocalPics provides Pics.Local) {
                    if (transcript.isEmpty())
                        Hint(
                            if (loading) "読み込み中…" else if (has) "作業記録はまだありません" else "作業記録は提供されていません"
                        )
                    for (row in transcript) key(row.key) {
                        captions[row.key]?.let { Hint(it) }
                        var expanded by remember { mutableStateOf(false) }
                        RowView(row, expanded) { expanded = !expanded }
                    }
                }
                if (child.str("state") == "waiting") {
                    val question = child.str("question")
                    Hint(if (question.isEmpty()) "確認が必要です" else "親の Pi に質問しています")
                    if (question.isNotEmpty()) Text(question)
                    TextAction(
                        if (question.isEmpty()) "チャットで確認" else "チャットで回答",
                        {
                            if (at == m.edits.current && at.target == m.target) {
                                m.edits.text(at, childAnswerDraft(child, m.edits.body.text))
                                close()
                            }
                        },
                        enabled = at == m.edits.current && at.target == m.target,
                    )
                }
                child.str("error").takeIf(String::isNotEmpty)?.let { SheetError(it) }
                if (record.truncated) Hint("記録の一部を省略しています")
                SheetError(error)
                if (record.more || error.isNotEmpty() && has)
                    TextAction("記録を読み込む", { retry++ }, enabled = !loading)
                val actions =
                    child.optJSONArray("actions").strings().filter {
                        it in listOf("steer", "resume")
                    }
                var busy by remember { mutableStateOf(false) }
                if (actions.isEmpty())
                    Hint(
                        if ("follow_up" in child.optJSONArray("actions").strings())
                            "この子は割り込み未対応です。記録を見るだけです。"
                        else "この子の作業記録は見るだけです。"
                    )
                else {
                    var text by remember { mutableStateOf("") }
                    var mode by remember(actions) { mutableStateOf(actions.first()) }
                    Label("子への指示")
                    if (actions.size > 1)
                        Segment(actions.map { it to agentAction(it) }, mode) { mode = it }
                    OutlinedTextField(
                        text,
                        { text = it },
                        Modifier.fillMaxWidth(),
                        label = { Text("指示") },
                        enabled = !busy,
                    )
                    PrimaryButton(
                        agentAction(mode),
                        {
                            if (!busy) {
                                busy = true
                                scope.launch {
                                    try {
                                        checked({ error = it }) {
                                            val r = m.control(at, rid, cid, mode, text)
                                            text = ""
                                            retry++
                                            if (
                                                mode == "resume" && r.str("newRunId").isNotEmpty()
                                            ) {
                                                resumedIndex = child.optInt("index", -1)
                                                resumedTo = r.str("newRunId")
                                            }
                                        }
                                    } finally {
                                        busy = false
                                    }
                                }
                            }
                        },
                        enabled = !busy && text.isNotBlank(),
                    )
                }
                StopAgent(m, at, run, child)
            }
        }
}

internal data class StopTarget(val run: String, val viewedChild: String?, val action: String) {
    val child
        get() = viewedChild.takeIf { action == "stop_child" }
}

internal fun JSONObject.stopTarget(childId: String?): StopTarget? {
    if (!optBoolean("active")) return null
    val action =
        if (childId == null) str("stopAction")
        else {
            val c =
                optJSONArray("children").objects().find { it.str("id") == childId } ?: return null
            if (c.str("state") !in setOf("running", "waiting", "queued")) return null
            if ("stop_child" in c.optJSONArray("actions").strings()) "stop_child"
            else str("stopAction")
        }
    return if (action.isEmpty()) null else StopTarget(str("id"), childId, action)
}

@Composable
private fun StopAgent(m: SessionModel, at: EditorKey, run: JSONObject, child: JSONObject?) {
    val runId = run.str("id")
    StopAgentAction(
        at,
        run.stopTarget(child?.str("id")),
        latest = {
            m.rows.groups["run"]
                .orEmpty()
                .find { it.id == runId }
                ?.let { JSONObject(it.json).stopTarget(child?.str("id")) }
        },
    ) { approved ->
        m.control(at, approved.run, approved.child, approved.action)
    }
}

@Composable
internal fun StopAgentAction(
    at: EditorKey,
    target: StopTarget?,
    latest: () -> StopTarget?,
    send: suspend (StopTarget) -> Unit,
) {
    if (target == null) return
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val label = agentAction(target.action)
    ConfirmAction(
        at to target,
        label,
        when (target.action) {
            "abort_parent" -> "親の Pi のターンも停止します。"
            "stop_child" -> "選択した子だけが対象です。"
            else -> "この実行に含まれるすべての子が対象です。"
        },
        button = "$label…",
        enabled = !busy,
    ) {
        scope.launch {
            // A revision can arrive between the last composition and dispatch.
            if (target != latest()) return@launch
            busy = true
            try {
                checked({ error = it }) { send(target) }
            } finally {
                busy = false
            }
        }
    }
    SheetError(error)
}

@Composable
private fun AgentLink(title: String, detail: String, state: String, open: () -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .clickable(role = androidx.compose.ui.semantics.Role.Button, onClick = open)
            .heightIn(min = 56.dp)
            .padding(vertical = 6.dp),
        verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
    ) {
        Dot(if (state == "失敗") pi.failed else if (state == "入力待ち") pi.waiting else pi.done)
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = Type.lead)
            Hint(detail)
        }
        Hint(state)
    }
}
