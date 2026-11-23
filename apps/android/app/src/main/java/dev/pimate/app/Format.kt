package dev.pimate.app

import java.util.Calendar
import org.json.JSONArray
import org.json.JSONObject

// シートと入力欄の表記（TS 版の lib/sessionInfo.ts・commands.ts・treeRows.ts と同じ決め方）。Android の部品に依存しない。

internal fun cost(value: Double) =
    "$" + (if (value > 0 && value < .01) "%.3f" else "%.2f").format(value)

/** 「Claude Opus 5.5」→「Opus 5.5」（入力欄は短く、シートは正式な名前）。 */
internal fun shortModel(name: String) =
    name.replace(Regex("^Claude\\s+", RegexOption.IGNORE_CASE), "").trim().ifEmpty { name }

internal fun thinkingShort(level: String) =
    when (level) {
        "minimal" -> "min"
        "medium" -> "med"
        else -> level
    }

/**
 * 入力欄の「Opus 5.5 · high」。Thinking は off と、段階のないモデルでは出さない。models が未取得なら ID をそのまま出す。 levels: そのモデルの
 * thinkingLevels（不明なら null）。
 */
internal fun modelLabel(id: String, name: String?, thinking: String, levels: Int?): String {
    val shown = name?.let(::shortModel) ?: id
    val level = thinking.isNotEmpty() && thinking != "off" && (levels == null || levels > 1)
    return if (level) "$shown · $thinking" else shown
}

private val providers =
    mapOf(
        "anthropic" to "Anthropic",
        "openai" to "OpenAI",
        "google" to "Google",
        "openai-codex" to "OpenAI Codex",
        "opencode" to "OpenCode Zen",
        "opencode-go" to "OpenCode Go",
        "openrouter" to "OpenRouter",
        "xai" to "xAI",
        "mistral" to "Mistral",
        "groq" to "Groq",
    )

internal fun providerName(id: String) = providers[id] ?: id

/** 「82K / 200K · 41%」。Pi がまだ使用量を知らなければ「? / 200K」。 */
internal fun contextLine(context: JSONObject?): String? {
    if (context == null) return null
    val window = count(context.optDouble("contextWindow", 0.0)).uppercase()
    if (context.isNull("tokens")) return "? / $window"
    val percent =
        if (context.isNull("percent")) "" else " · ${Math.round(context.optDouble("percent"))}%"
    return "${count(context.optDouble("tokens")).uppercase()} / $window$percent"
}

/** 300 → 5時間、10080 → 週間。 */
internal fun windowLabel(minutes: Long) =
    when {
        minutes == 7L * 24 * 60 -> "週間"
        minutes % (24 * 60) == 0L -> "${minutes / (24 * 60)}日"
        minutes % 60 == 0L -> "${minutes / 60}時間"
        else -> "${minutes}分"
    }

/** 今日なら「14:05」、ほかの日は「10/24 10:53」（端末のタイムゾーン）。 */
internal fun clockTime(at: Long, now: Long): String {
    val day = Calendar.getInstance().apply { timeInMillis = at }
    val today = Calendar.getInstance().apply { timeInMillis = now }
    val time = "%d:%02d".format(day[Calendar.HOUR_OF_DAY], day[Calendar.MINUTE])
    val same =
        day[Calendar.YEAR] == today[Calendar.YEAR] &&
            day[Calendar.DAY_OF_YEAR] == today[Calendar.DAY_OF_YEAR]
    return if (same) time else "${day[Calendar.MONTH] + 1}/${day[Calendar.DAY_OF_MONTH]} $time"
}

/** 「あと1時間25分」「あと3日4時間」「あと5分」。 */
internal fun timeLeft(at: Long, now: Long): String {
    val minutes = maxOf(1, (at - now + 59_999) / 60_000)
    val d = minutes / 1440
    val h = minutes % 1440 / 60
    val m = minutes % 60
    return when {
        d > 0 -> "あと${d}日" + if (h > 0) "${h}時間" else ""
        h > 0 -> "あと${h}時間" + if (m > 0) "${m}分" else ""
        else -> "あと${m}分"
    }
}

/** 利用枠の 1 行の右側。期限を過ぎた数字は古いので、出さずにそう書く。 */
internal fun resetLabel(resetsAt: Long?, now: Long) =
    when {
        resetsAt == null -> "リセット日時不明"
        resetsAt <= now -> "リセット済み・次の応答で更新"
        else -> "${clockTime(resetsAt, now)} にリセット（${timeLeft(resetsAt, now)}）"
    }

/** 80% から黄、95% から赤（使用状況のリングと棒）。 */
internal enum class Usage {
    Normal,
    High,
    Full,
}

internal fun usage(percent: Double?) =
    when {
        percent != null && percent >= 95 -> Usage.Full
        percent != null && percent >= 80 -> Usage.High
        else -> Usage.Normal
    }

// ---- / コマンド ----

/** 先頭の `/名前`（中核の slash_command_name と同じ文字の範囲）。 */
internal fun slashName(text: String): String? =
    text
        .split(Regex("\\s"), 2)
        .first()
        .takeIf { it.startsWith("/") }
        ?.drop(1)
        ?.takeIf { it.isNotEmpty() && it.all { c -> c.isLetterOrDigit() && c.code < 128 || c in "_:.-" } }

/** アプリの操作に置き換える組み込みコマンドの説明。 */
internal val builtinCommands =
    mapOf(
        "model" to "モデルと Thinking を選ぶ",
        "reload" to "Extension などを再読み込み",
        "new" to "新しいセッション",
        "resume" to "過去のセッションを開く",
        "fork" to "メッセージを選んで分岐",
        "compact" to "コンテキストを圧縮",
        "tree" to "セッションツリーを開く",
    )

internal fun commandKind(source: String) =
    when (source) {
        "builtin" -> "アプリ操作"
        "extension" -> "Extension"
        "prompt" -> "テンプレート"
        "skill" -> "Skill"
        else -> source
    }

// ---- セッションの切り替えの行（Chat-Edge） ----

/** 会話の先頭に出す 1 行目。byPhone: このスマホの操作で切り替わった。そうでなければ PC（か別の端末）。 */
internal fun switchLine(reason: String, byPhone: Boolean): Pair<String, String?> =
    if (byPhone)
        when (reason) {
            "new" -> "新しいセッションを開きました"
            "fork" -> "ここから分岐しました"
            else -> "セッションを開きました"
        } to null
    else
        when (reason) {
            "new" -> "PC のターミナルで" to "/new"
            "resume" -> "PC のターミナルで" to "/resume"
            "fork" -> "PC のターミナルで" to "/fork"
            else -> "会話が切り替わりました" to null
        }

// ---- セッションツリー ----

internal data class TreeRow(
    val id: String,
    val kind: String,
    val preview: String,
    val label: String,
    val time: Long?,
    val onBranch: Boolean,
    val current: Boolean,
    val leaf: Boolean,
    val depth: Int,
)

/** RC は行きがけ順で送る。絞り込みでは隠れた親を畳み、分かれ道だけ字下げする。 */
internal fun treeRows(nodes: JSONArray, usersOnly: Boolean): List<TreeRow> {
    class Node(val o: JSONObject, val parent: Node?) {
        var current = o.optBoolean("isCurrent")
        var children = 0
        var depth = 0
    }
    val rows = mutableListOf<Node>()
    val visible = HashMap<String, Node?>()
    for (i in 0 until nodes.length()) {
        val o = nodes.optJSONObject(i) ?: continue
        val parent = (o.opt("parentId") as? String)?.let { visible[it] }
        if (usersOnly && o.str("kind") != "user") {
            visible[o.str("id")] = parent
            if (o.optBoolean("isCurrent")) parent?.current = true
            continue
        }
        val node = Node(o, parent)
        rows += node
        visible[o.str("id")] = node
        parent?.let { it.children++ }
    }
    for (node in rows) node.parent?.let { node.depth = it.depth + if (it.children > 1) 1 else 0 }
    return rows.map {
        TreeRow(
            it.o.str("id"),
            it.o.str("kind"),
            it.o.str("preview"),
            it.o.str("label"),
            if (it.o.has("timestamp")) it.o.optLong("timestamp") else null,
            it.o.optBoolean("onBranch"),
            it.current,
            it.children == 0,
            it.depth,
        )
    }
}

internal fun treeKind(kind: String) =
    when (kind) {
        "user" -> "あなた"
        "assistant" -> "Pi"
        "compaction" -> "圧縮"
        "branch_summary" -> "枝の要約"
        "custom_message" -> "拡張メッセージ"
        else -> kind
    }
