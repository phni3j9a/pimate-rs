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

@Composable
internal fun ReviewPage(m: SessionModel, at: EditorKey, close: () -> Unit) {
    val items = m.rows.groups["entry"].orEmpty()
    val cwd =
        m.rows.groups["instance"]
            .orEmpty()
            .find { it.id == at.target.instance }
            ?.let { JSONObject(it.json).str("cwd") }
            .orEmpty()
    var all by rememberSaveable { mutableStateOf(false) }
    var turn by rememberSaveable { mutableStateOf<String?>(null) }
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
                    entries
                        .filter { it.str("kind") == "user" }
                        .map { it.str("entryId") to it.str("text").take(80) }
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
                    if (expanded)
                        file.pieces.forEach { piece ->
                            SelectionContainer {
                                Column(
                                    Modifier.fillMaxWidth()
                                        .background(pi.surface, RoundedCornerShape(12.dp))
                                        .horizontalScroll(rememberScrollState())
                                        .padding(10.dp)
                                ) {
                                    if (piece.diff?.hunks?.isNotEmpty() == true)
                                        piece.diff.hunks.forEach { h ->
                                            Text(
                                                h.header,
                                                style = Type.mono,
                                                color = pi.textSecondary,
                                            )
                                            h.lines.forEach { line ->
                                                val add = line.old == null
                                                val del = line.new == null
                                                Text(
                                                    "${line.old ?: " "}\t${line.new ?: " "}  ${if(add) "+" else if(del) "−" else " "} ${line.text}",
                                                    style = Type.mono,
                                                    color =
                                                        if (add) pi.addText
                                                        else if (del) pi.delText else pi.text,
                                                )
                                            }
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
                                        (piece.content ?: piece.patch)?.let {
                                            Text(it, style = Type.mono)
                                        }
                                    }
                                }
                            }
                        }
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
            data = m.github(at, q).getJSONObject("data")
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
                    MenuRow(
                        "#${v.optInt("number")}　${v.str("title")}",
                        v.str("state"),
                        true,
                        next = true,
                    ) {
                        number = v.getInt("number")
                    }
                }
            } else
                r.optJSONObject("item")?.let { v ->
                    CompositionLocalProvider(LocalPics provides Pics.Local) {
                        Text(v.str("title"), style = Type.title)
                        Hint("${v.str("author")} · ${v.str("state")}")
                        v.optJSONArray("checks").objects().forEach {
                            Hint("${it.str("name")} · ${it.str("state")}")
                        }
                        Markdown(v.str("body").ifEmpty { "本文はありません" })
                        if (v.optBoolean("bodyTruncated")) Hint("本文の一部を省略しています")
                        Label("コメント")
                        v.optJSONArray("comments").objects().forEach {
                            Text(it.str("author"), style = Type.header)
                            Markdown(it.str("body"))
                            if (it.optBoolean("bodyTruncated")) Hint("一部を省略しています")
                        }
                        if (v.optBoolean("commentsTruncated")) Hint("最新のコメントのみ表示しています")
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

@Composable
internal fun AgentsPage(m: SessionModel, at: EditorKey, close: () -> Unit) {
    var rid by rememberSaveable { mutableStateOf("") }
    var cid by rememberSaveable { mutableStateOf("") }
    val rows = m.rows.groups["run"].orEmpty()
    val runs = remember(rows) { rows.map { JSONObject(it.json) } }
    if (cid.isEmpty())
        PiSheet(close) {
            SheetTitle("サブエージェント")
            for (open in listOf(true, false)) {
                val group = runs.filter { it.optBoolean("open") == open }
                if (group.isNotEmpty()) Label(if (open) "実行中・再開可能" else "以前の実行")
                group.forEach { run ->
                    key(run.str("id")) {
                        Hint("${run.str("backend")} · ${run.str("mode")}")
                        run.optJSONArray("children").objects().forEach { c ->
                            val detail =
                                listOfNotNull(
                                        c.str("agent"),
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
            Page(child?.str("title").orEmpty().ifEmpty { "サブエージェント" }, { cid = "" }) {
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
                val labels =
                    m.instructions
                        .forChild(ChildKey(at, rid, cid), run.str("resumedFrom").ifEmpty { null })
                        .toMutableList()
                if (record.items.isEmpty()) Hint(if (has) "作業記録はまだありません" else "作業記録は提供されていません")
                record.items.forEach { v ->
                    val text = v.str("text")
                    when (v.str("kind")) {
                        "user" -> {
                            Hint(
                                labels
                                    .indexOfFirst { it.text.trim() == text.trim() }
                                    .takeIf { it >= 0 }
                                    ?.let(labels::removeAt)
                                    ?.let { "あなた · ${agentAction(it.action)}" } ?: "依頼"
                            )
                            Text(text)
                        }
                        "assistant" -> {
                            v.str("thinking").takeIf(String::isNotEmpty)?.let { Hint(it) }
                            v.optJSONArray("toolCalls").objects().forEach {
                                Text(it.str("name"), style = Type.mono)
                            }
                            if (text.isNotEmpty()) Markdown(text)
                        }
                        else -> {
                            Hint(v.str("toolName"))
                            Text(text, style = Type.mono)
                        }
                    }
                }
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
                                            if (mode == "resume" && r.str("newRunId").isNotEmpty())
                                                rid = r.str("newRunId")
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

@Composable
private fun StopAgent(m: SessionModel, at: EditorKey, run: JSONObject, child: JSONObject?) {
    val action =
        if (child?.optJSONArray("actions").strings().contains("stop_child")) "stop_child"
        else run.str("stopAction")
    if (action.isEmpty()) return
    var confirm by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    TextAction(
        agentAction(action) + "…",
        { confirm = true },
        enabled = !busy,
        color = pi.failedText,
    )
    SheetError(error)
    if (confirm)
        Confirm(
            agentAction(action),
            when (action) {
                "abort_parent" -> "親の Pi のターンも停止します。"
                "stop_child" -> "選択した子だけが対象です。"
                else -> "この実行に含まれるすべての子が対象です。"
            },
            { confirm = false },
        ) {
            confirm = false
            busy = true
            scope.launch {
                try {
                    checked({ error = it }) {
                        m.control(
                            at,
                            run.str("id"),
                            if (action == "stop_child") child?.str("id") else null,
                            action,
                        )
                    }
                } finally {
                    busy = false
                }
            }
        }
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
