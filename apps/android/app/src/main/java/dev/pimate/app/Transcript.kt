package dev.pimate.app

import java.nio.ByteBuffer
import java.security.MessageDigest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.withContext
import org.json.JSONObject
import uniffi.pimate_ffi.ImageKind

// 状態の層（Rows）の JSON を画面の行にする。組み立ては JSON を知らない buildRows が行う。

internal data class Call(
    val id: String,
    val name: String,
    val subject: String,
    val writeContent: String? = null,
)

/** 表示する画像の出どころ。id はメモリ内のキャッシュの名前（空ならキャッシュしない）。 */
internal sealed interface Pic {
    val id: String

    /** 状態の層がすでに持っている画像（base64）。 */
    data class Inline(override val id: String, val mime: String, val data: String) : Pic

    /** 履歴の発言・ツール結果の画像。entry_image で取りに行く。 */
    data class Entry(val entryId: String, val index: Int) : Pic {
        override val id
            get() = "e:$entryId:$index"
    }

    /** PC のファイル。file_image で取りに行く。 */
    data class File(val path: String, val answer: String? = null) : Pic {
        override val id
            get() = "" // PC のファイルは回答間で上書きされる。
    }

    /** Markdown は表示領域に入るまで大きな共有文字列を展開しない。 */
    class Shared(
        private val kind: ImageKind,
        private val source: uniffi.pimate_ffi.SharedTextInterface,
        private val answer: String? = null,
    ) : Pic {
        override val id = ""

        fun forAnswer(answer: String) = Shared(kind, source, answer)

        suspend fun resolve(): Pic =
            withContext(Dispatchers.Default) {
                val src = source.value()
                when (kind) {
                    ImageKind.REMOTE -> Web(src)
                    ImageKind.FILE -> File(src, answer)
                    ImageKind.DATA ->
                        Inline(
                            "",
                            src.substringBefore(';').removePrefix("data:"),
                            src.substringAfter(','),
                        )
                }
            }
    }

    /** http(s)。スマホから直接取りに行く。 */
    data class Web(val url: String) : Pic {
        override val id
            get() = "w:$url"
    }
}

/** 表示中のタイル1個の中だけで比較する。再解析で共有ハンドルが変わっても同じ参照先は読み直さない。 */
internal fun Flow<Pic>.tileSources(): Flow<Pic?> =
    map { shared ->
            val src =
                try {
                    if (shared is Pic.Shared) shared.resolve() else shared
                } catch (e: CancellationException) {
                    throw e
                } catch (_: Exception) {
                    null
                }
            // 比較のために大きな base64 本文を保持しない。Web/PC は参照先の文字列だけ。
            val key =
                if (shared is Pic.Shared && src is Pic.Inline)
                    withContext(Dispatchers.Default) {
                        src.mime to
                            ByteBuffer.wrap(
                                MessageDigest.getInstance("SHA-256").digest(src.data.toByteArray())
                            )
                    }
                else src
            key to src
        }
        .distinctUntilChangedBy { it.first }
        .map { it.second }

internal data class Entry(
    val id: String,
    val kind: String, // user / assistant / tool_result / other
    val text: String = "",
    val thinking: String = "",
    val calls: List<Call> = emptyList(),
    val final: Boolean = true, // assistant: この発言でターンが終わった（stopReason が toolUse 以外）
    val error: String = "",
    val aborted: Boolean = false,
    val callId: String = "", // tool_result
    val name: String = "",
    val isError: Boolean = false,
    val change: Piece? = null,
    val time: Long? = null,
    /** user / tool_result の画像を元の位置で。プレビューが省かれた位置は Pic.Entry（取りに行く）。 */
    val pics: ImageSlots = ImageSlots.Empty,
)

/** 実行中のツール（"tool" の行）。履歴に結果が載るまでの表示に使う。 */
internal data class Live(
    val id: String,
    val name: String,
    val subject: String,
    val status: String,
    val output: String,
    val durationMs: Long?,
    val nested: Boolean,
    val startedAt: Long? = null,
)

internal enum class ToolState(val label: String) {
    Running("実行中"),
    Done("完了"),
    Failed("エラー"),
}

internal sealed interface ChatRow {
    val key: String
    val expansionKey: String
        get() = key

    /** 畳まれるターンの要約行の key。開かれるまで表示しない。 */
    val fold: String?

    data class User(
        override val key: String,
        val text: String,
        val pics: ImageSlots = ImageSlots.Empty,
    ) : ChatRow {
        override val fold: String?
            get() = null
    }

    data class Summary(
        override val key: String,
        val text: String,
        val reviewTurn: String? = null,
        val reviewLabel: String = "",
    ) : ChatRow {
        override val fold: String?
            get() = null
    }

    data class Thinking(
        override val key: String,
        val text: String,
        val live: Boolean,
        override val fold: String? = null,
        /** 考えた時間。この端末が見ていた発言だけ分かる。 */
        val ms: Long? = null,
        override val expansionKey: String = key,
    ) : ChatRow

    data class Text(override val key: String, val text: String, override val fold: String? = null) :
        ChatRow

    data class Tool(
        override val key: String,
        val name: String,
        val subject: String,
        val state: ToolState,
        val output: String,
        val meta: String,
        override val fold: String? = null,
        val pics: ImageSlots = ImageSlots.Empty,
        /** 実行中のツールの開始時刻（epoch ms）。分からなければ秒数を出さない。 */
        val since: Long? = null,
        val change: Piece? = null,
    ) : ChatRow

    /** 会話の切り替え（PC の /resume など）。履歴にはない行で、切り替えを見た位置に置く。 */
    data class Switch(val reason: String, val byPhone: Boolean, val from: String, val to: String) :
        ChatRow {
        override val key
            get() = "switched"

        override val fold: String?
            get() = null
    }

    /** tone: note（補足）/ error（失敗）。 */
    data class Note(
        override val key: String,
        val text: String,
        val error: Boolean = false,
        override val fold: String? = null,
    ) : ChatRow
}

internal fun thinkingLabel(live: Boolean, ms: Long?): String {
    if (live) return "Thinking…"
    if (ms == null || ms < 1000) return "Thought"
    val s = (ms + 500) / 1000
    return "Thought for " + if (s < 60) "${s}s" else "${s / 60}m ${s % 60}s"
}

internal fun seconds(ms: Long): String {
    val s = (ms + 500) / 1000
    return if (s < 60) "${s}秒" else "${s / 60}分${s % 60}秒"
}

internal fun elapsed(ms: Long): String {
    val s = ms / 1000
    val tail = "%02d".format(s % 60)
    return if (s >= 3600) "${s / 3600}:${"%02d".format(s % 3600 / 60)}:$tail" else "${s / 60}:$tail"
}

private fun lines(text: String) =
    if (text.isEmpty()) 0 else text.removeSuffix("\n").count { it == '\n' } + 1

/**
 * 完了したターンの作業（考えた内容・ツール・途中の発言）は要約行 1 行に畳み、最後の回答・エラー・補足は残す。
 * 進行中のターンと、ツールを使っていないターンは畳まない（docs/SPEC.md「情報密度」）。
 */
internal fun buildRows(
    entries: List<Entry>,
    live: List<Live>,
    active: Boolean,
    streamText: String = "",
    streamThinking: String = "",
    thinkingMs: Map<String, Long> = emptyMap(),
    liveThinkingMs: Long? = null,
): List<ChatRow> {
    // 結果は、同じ ID でまだ結果のない直前の呼び出しに対応付ける（ID がターンをまたいで重なる履歴でも取り違えない）。
    val results = HashMap<Pair<String, String>, Entry>() // (呼び出し元 entry, call ID) → 結果
    val claimed = HashSet<Int>() // 呼び出しに対応した結果の位置
    val caller = HashMap<String, String>() // call ID → 最後に呼んだ entry。実行中のツールはここに付ける
    run {
        val open = HashMap<String, String>()
        entries.forEachIndexed { i, e ->
            if (e.kind == "assistant")
                e.calls.forEach {
                    open[it.id] = e.id
                    caller[it.id] = e.id
                }
            else if (e.kind == "tool_result")
                open.remove(e.callId)?.let {
                    results[it to e.callId] = e
                    claimed += i
                }
        }
    }
    val running = live.filterNot { it.nested }.associateBy { it.id }
    val out = mutableListOf<ChatRow>()
    var turn = mutableListOf<ChatRow>()
    var turnId: String? = null
    var started: Long? = null
    var ended: Long? = null
    var finalKey: String? = null

    fun tool(
        key: String,
        name: String,
        subject: String,
        result: Entry?,
        now: Live?,
        writeContent: String? = null,
    ): ChatRow.Tool {
        val state =
            when {
                result != null -> if (result.isError) ToolState.Failed else ToolState.Done
                now?.status == "error" -> ToolState.Failed
                now?.status == "running" || now == null && active -> ToolState.Running
                else -> ToolState.Done
            }
        val shownName = name.ifEmpty { now?.name ?: "tool" }
        val piece = result?.change?.withWriteContent(writeContent)
        val diff = piece?.diff
        val meta =
            when {
                state == ToolState.Failed -> "エラー"
                state == ToolState.Running -> ""
                diff != null -> "+${diff.add} −${diff.del}"
                result != null && shownName == "read" -> "${lines(result.text)} 行"
                now?.durationMs != null && now.durationMs >= 1000 -> seconds(now.durationMs)
                else -> ""
            }
        return ChatRow.Tool(
            key,
            shownName,
            subject.ifEmpty { now?.subject.orEmpty() },
            state,
            result?.text ?: now?.output.orEmpty(),
            meta,
            change = piece,
            pics = result?.pics ?: ImageSlots.Empty,
            since = now?.startedAt?.takeIf { state == ToolState.Running },
        )
    }

    fun close(inProgress: Boolean) {
        val id = turnId
        val tools = turn.count { it is ChatRow.Tool }
        if (id == null || inProgress || tools == 0) out += turn
        else {
            val key = "turn:$id"
            val took = started?.let { s -> ended?.let { it - s } }?.takeIf { it >= 1000 }
            var summarized = false
            for (row in turn) {
                val folded: ChatRow? =
                    when {
                        row is ChatRow.Thinking -> row.copy(fold = key)
                        row is ChatRow.Tool && row.pics.size == 0 -> row.copy(fold = key)
                        row is ChatRow.Text && row.key != finalKey -> row.copy(fold = key)
                        else -> null
                    }
                if (folded != null && !summarized) {
                    summarized = true
                    val changes = turn.filterIsInstance<ChatRow.Tool>().filter { it.change != null }
                    val diffs = changes.mapNotNull { it.change?.diff }
                    out +=
                        ChatRow.Summary(
                            key,
                            listOfNotNull(took?.let { "${seconds(it)} 作業" }, "ツール $tools 回")
                                .joinToString(" · "),
                            id.takeIf { changes.isNotEmpty() },
                            if (diffs.isEmpty()) "${changes.distinctBy { it.subject }.size} ファイル ›"
                            else "+${diffs.sumOf { it.add }} −${diffs.sumOf { it.del }} ›",
                        )
                }
                out += folded ?: row
            }
        }
        turn = mutableListOf()
    }

    entries.forEachIndexed { i, e ->
        if (e.kind != "user" && e.time != null) ended = e.time
        when (e.kind) {
            "user" -> {
                close(false)
                turnId = e.id
                started = e.time
                ended = null
                finalKey = null
                turn += ChatRow.User(e.id, e.text, e.pics)
            }
            "assistant" -> {
                if (e.thinking.isNotBlank())
                    turn +=
                        ChatRow.Thinking(
                            "${e.id}:thinking",
                            e.thinking,
                            false,
                            ms = thinkingMs[e.id],
                            expansionKey = "thinkingAfter:${entries.getOrNull(i - 1)?.id.orEmpty()}",
                        )
                if (e.text.isNotBlank()) {
                    turn += ChatRow.Text("${e.id}:text", e.text)
                    if (e.final) finalKey = "${e.id}:text"
                }
                e.calls.forEach {
                    turn +=
                        tool(
                            "tool:${e.id}:${it.id}",
                            it.name,
                            it.subject,
                            results[e.id to it.id],
                            running[it.id]?.takeIf { _ -> caller[it.id] == e.id },
                            it.writeContent,
                        )
                }
                if (e.aborted) turn += ChatRow.Note("${e.id}:aborted", "中断しました")
                else if (e.error.isNotEmpty())
                    turn += ChatRow.Note("${e.id}:error", "エラー: ${e.error}", true)
            }
            // 呼び出しが履歴にない結果（途中からの履歴など）。
            "tool_result" -> if (i !in claimed) turn += tool("result:${e.id}", e.name, "", e, null)
            else -> {
                val first = e.text.trim().lineSequence().firstOrNull().orEmpty()
                turn +=
                    ChatRow.Note(
                        e.id,
                        (if (first.isEmpty()) e.name else "${e.name}: $first").take(160),
                    )
            }
        }
    }
    // 実行後に PC 拡張が開いた質問は、完了ターンを再び展開する理由にならない。
    close(active)
    // 途中から開いたときなど、発言より先に届いた実行中のツール。サブエージェントの中のツールは出さない。
    running.values
        .filter { it.id !in caller }
        .forEach { out += tool("running:${it.id}", it.name, it.subject, null, it) }
    if (streamThinking.isNotBlank())
        out +=
            ChatRow.Thinking(
                "live:thinking",
                streamThinking,
                streamText.isEmpty(),
                ms = liveThinkingMs,
                expansionKey = "thinkingAfter:${entries.lastOrNull()?.id.orEmpty()}",
            )
    if (streamText.isNotEmpty()) out += ChatRow.Text("live:text", streamText)
    // 同じ ID が重なる壊れた履歴でも、リストの key の重複で落とさない。
    return out.distinctBy { it.key }
}

/** 行の間の余白（dp）。ターンの間 32、ツールの行は詰める（docs/design/html/Main.html）。 */
internal fun gap(prev: ChatRow?, row: ChatRow): Int =
    when {
        prev == null -> 0
        prev is ChatRow.Switch || row is ChatRow.Switch -> 28
        row is ChatRow.User -> if (prev is ChatRow.User) 12 else 32
        row is ChatRow.Summary -> 10
        prev is ChatRow.Summary -> if (row is ChatRow.Text) 6 else 4
        prev is ChatRow.User -> if (row is ChatRow.Text) 10 else 12
        row is ChatRow.Tool || row is ChatRow.Thinking ->
            if (prev is ChatRow.Tool || prev is ChatRow.Thinking) 0 else 8
        row is ChatRow.Text -> if (prev is ChatRow.Text) 12 else 8
        else -> 12
    }

/**
 * 切り替えの行を置く位置: 切り替えを見たときの最後の発言（anchor）より後に始まったターンの前。 そういうターンがまだなければ末尾（いちばん新しい位置）。anchor が
 * null（空の会話に切り替わった）なら先頭。
 */
internal fun placeSwitch(
    rows: List<ChatRow>,
    note: ChatRow.Switch,
    entries: List<Entry>,
    anchor: String?,
    known: Boolean,
): List<ChatRow> {
    val after = if (!known) entries.lastIndex else entries.indexOfFirst { it.id == anchor }
    val order = entries.withIndex().associate { it.value.id to it.index }
    val at =
        rows
            .indexOfFirst { it is ChatRow.User && (order[it.key] ?: -1) > after }
            .let { if (it < 0) rows.size else it }
    return rows.subList(0, at) + note + rows.subList(at, rows.size)
}

/** 中核が再接続をやめた接続（待っても戻らない）。repair: 再ペアリングで直る。そうでなければ更新してから再接続する。 */
internal enum class Stop(val label: String, val detail: String, val repair: Boolean) {
    Revoked("登録解除", "PC でこのスマホの登録が解除されました。使うには、もう一度ペアリングしてください。", true),
    InvalidToken("再ペアリングが必要", "このスマホの登録が PC にありません。もう一度ペアリングしてください。", true),
    AuthFailed("認証できません", "PC を確認できませんでした。PC の PiMate を入れ直したときなどは、もう一度ペアリングしてください。", true),
    Protocol("バージョン不一致", "PC の PiMate とアプリのバージョンが合いません。どちらかを更新してから再接続してください。", false);

    val action
        get() = if (repair) "再ペアリング" else "再接続"
}

/** 状態の層の host.connection（JSON の文字列）から。接続中・一時的な切断は null。 */
internal fun stopped(connection: String?): Stop? =
    when (connection) {
        "\"revoked\"" -> Stop.Revoked
        "\"invalid_token\"" -> Stop.InvalidToken
        "\"auth_failed\"" -> Stop.AuthFailed
        "\"protocol_mismatch\"" -> Stop.Protocol
        else -> null
    }

/** 会話の欄に会話の代わりに出す案内と、「一覧を開く」を添えるか。null なら会話を出す。 履歴が空でも、質問や PC 側の操作待ちがあれば会話の欄で示す。 */
internal fun notice(
    selected: Boolean,
    phase: String?,
    ready: Boolean,
    history: Boolean,
    asking: Boolean,
    pcPrompt: Boolean,
    offline: Boolean = false,
): Pair<String, Boolean>? =
    when {
        !selected -> "一覧から Pi を選ぶと、ここに会話が表示されます" to true
        // 切断中は、中核が会話を持っていない（つながり直すと読み直す）。
        offline -> "PC に再接続しています。書きかけの下書きは残ります。" to false
        phase == "\"instance_gone\"" -> "この Pi は終了しました" to true
        phase == "\"subscribe_error\"" -> "この Pi を開けませんでした" to true
        !ready -> "読み込み中…" to false
        !history && !asking && !pcPrompt -> "メッセージを送ると、ここに会話が表示されます" to false
        else -> null
    }

// ---- JSON（状態の層の行）から ----

/** org.json の optString は JSON の null を "null" にするので使わない。 */
internal fun JSONObject.str(key: String) = opt(key) as? String ?: ""

private fun firstLine(text: String) = text.trim().lineSequence().firstOrNull().orEmpty().take(200)

/** ツール名の隣に出す短い対象: パス・コマンド・パターン。 */
internal fun subject(name: String, args: JSONObject?): String {
    if (args == null) return ""
    fun str(key: String) = (args.opt(key) as? String)?.takeIf { it.isNotEmpty() }
    val path = str("path") ?: str("file_path") ?: str("filePath")
    return when (name) {
        "bash" -> firstLine(str("command").orEmpty())
        "read",
        "edit",
        "write",
        "ls" -> path.orEmpty()
        "grep",
        "find" -> listOfNotNull(str("pattern"), path).joinToString("  ")
        else ->
            path
                ?: args
                    .keys()
                    .asSequence()
                    .mapNotNull { str(it)?.trim() }
                    .firstOrNull { it.isNotEmpty() }
                    ?.let(::firstLine)
                    .orEmpty()
    }
}

internal fun count(n: Double): String =
    when {
        n < 1000 -> n.toLong().toString()
        n < 10_000 -> "%.1fk".format(n / 1000).replace(".0k", "k")
        n < 1_000_000 -> "${Math.round(n / 1000)}k"
        else -> "%.1fM".format(n / 1_000_000).replace(".0M", "M")
    }

internal fun entry(o: JSONObject): Entry {
    val id = o.str("entryId")
    val time = if (!o.isNull("timestamp")) o.optLong("timestamp") else null
    return when (val kind = o.str("kind")) {
        "assistant" -> {
            val calls = o.optJSONArray("toolCalls")
            val stop = o.str("stopReason")
            val error = o.str("errorMessage")
            Entry(
                id,
                kind,
                o.str("text"),
                o.str("thinking"),
                List(calls?.length() ?: 0) {
                    val c = calls!!.getJSONObject(it)
                    Call(
                        c.str("id"),
                        c.str("name"),
                        subject(c.str("name"), c.optJSONObject("args")),
                        if (c.str("name") == "write")
                            c.optJSONObject("args")?.opt("content") as? String
                        else null,
                    )
                },
                final = stop != "toolUse",
                error = error,
                // Pi は停止を stopReason "error" + "… was aborted" と記録することがある。停止は失敗ではない。
                aborted =
                    stop == "aborted" ||
                        Regex("(operation|request) was aborted", RegexOption.IGNORE_CASE)
                            .containsMatchIn(error),
                time = time,
            )
        }
        "tool_result" -> {
            Entry(
                id,
                kind,
                o.str("text"),
                callId = o.str("toolCallId"),
                name = o.str("toolName"),
                isError = o.optBoolean("isError"),
                change = resultPiece(o.str("toolName"), o),
                time = time,
                pics = pics(id, o),
            )
        }
        "user" -> Entry(id, kind, o.str("text"), time = time, pics = pics(id, o))
        else ->
            Entry(id, "other", o.str("text"), name = o.str("label").ifEmpty { kind }, time = time)
    }
}

/**
 * プレビューの配列の位置は、元の画像の位置とは限らない（省かれた画像がある）。imageIndices で元の位置に置き、 省かれた位置は取りに行く画像にする（TS 版の imageSlots
 * と同じ）。
 */
private fun pics(entryId: String, o: JSONObject): ImageSlots {
    val images = o.optJSONArray("images")
    val shown = images?.length() ?: 0
    val count =
        o.optLong("imageCount", shown.toLong())
            .coerceIn(shown.toLong(), Int.MAX_VALUE.toLong())
            .toInt()
    if (count == 0) return ImageSlots.Empty
    val slots = sortedMapOf<Int, Pic>()
    val indices = o.optJSONArray("imageIndices")
    for (i in 0 until shown) {
        val image = images!!.optJSONObject(i) ?: continue
        val at = indices?.optInt(i, -1) ?: i
        if (at in 0 until count)
            slots[at] = Pic.Inline("e:$entryId:$at", image.str("mimeType"), image.str("data"))
    }
    return ImageSlots(entryId, count, slots)
}

/** プレビューだけを保持し、欠番の参照はページを開いたときに作る。 */
internal class ImageSlots(
    private val entryId: String,
    val size: Int,
    private val previews: Map<Int, Pic>,
) {
    fun gallery(canLoad: Boolean): List<Pic> {
        if (!canLoad) return previews.values.toList()
        return object : AbstractList<Pic>() {
            override val size
                get() = this@ImageSlots.size

            override fun get(index: Int): Pic {
                if (index !in indices) throw IndexOutOfBoundsException()
                return previews[index] ?: Pic.Entry(entryId, index)
            }
        }
    }

    companion object {
        val Empty = ImageSlots("", 0, emptyMap())
    }
}

internal fun live(id: String, o: JSONObject): Live {
    // partial / 結果は { content: [{ type: "text", text }] }。
    val content = o.optJSONObject("partial")?.optJSONArray("content")
    val output = StringBuilder()
    for (i in 0 until (content?.length() ?: 0)) content!!
        .optJSONObject(i)
        ?.takeIf { it.str("type") == "text" }
        ?.let { output.append(it.str("text")) }
    val name = o.str("toolName")
    return Live(
        id,
        name,
        subject(name, o.optJSONObject("args")),
        o.str("status"),
        output.toString(),
        if (!o.isNull("durationMs")) o.optLong("durationMs") else null,
        o.has("parentToolCallId"),
        if (!o.isNull("startedAt")) o.optLong("startedAt") else null,
    )
}
