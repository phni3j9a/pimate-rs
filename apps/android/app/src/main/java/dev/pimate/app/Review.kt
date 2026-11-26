package dev.pimate.app

import org.json.JSONArray
import org.json.JSONObject

internal fun JSONArray?.objects(): List<JSONObject> =
    List(this?.length() ?: 0) { this?.optJSONObject(it) }.filterNotNull()

internal data class DiffLine(
    val old: Long?,
    val new: Long?,
    val text: String,
    var noNewline: Boolean = false,
)

internal data class Hunk(val header: String, val lines: List<DiffLine>)

internal data class Patch(val path: String?, val hunks: List<Hunk>) {
    val add = hunks.sumOf { h -> h.lines.count { it.old == null } }
    val del = hunks.sumOf { h -> h.lines.count { it.new == null } }
}

private val hunkHeader = Regex("^@@ -(\\d+)(?:,(\\d+))? \\+(\\d+)(?:,(\\d+))? @@")

internal fun parsePatch(text: String): Patch {
    val hunks = mutableListOf<Hunk>()
    var path: String? = null
    var git = false
    val lines = text.lines()
    var i = 0
    while (i < lines.size) {
        val line = lines[i++]
        if (line.startsWith("diff --git ")) git = true
        if (line.startsWith("--- ") && lines.getOrNull(i)?.startsWith("+++ ") == true) {
            val new = lines[i++].drop(4).substringBefore('\t')
            val old = line.drop(4).substringBefore('\t')
            val name = new.takeUnless { it == "/dev/null" } ?: old
            if (path == null)
                path = if (git) name.removePrefix(if (name == new) "b/" else "a/") else name
        }
        val match = hunkHeader.find(line) ?: continue
        var old = match.groupValues[1].toLongOrNull() ?: continue
        var new = match.groupValues[3].toLongOrNull() ?: continue
        var left = match.groups[2]?.value?.toLongOrNull() ?: 1
        var right = match.groups[4]?.value?.toLongOrNull() ?: 1
        val body = mutableListOf<DiffLine>()
        hunks += Hunk(line, body)
        while (i < lines.size) {
            val raw = lines[i]
            val content = raw.drop(1)
            when {
                raw.startsWith("\\") -> body.lastOrNull()?.noNewline = true
                raw.startsWith("+") && right > 0 -> {
                    body += DiffLine(null, new++, content)
                    right--
                }
                raw.startsWith("-") && left > 0 -> {
                    body += DiffLine(old++, null, content)
                    left--
                }
                (raw.startsWith(" ") || raw.isEmpty()) && left > 0 && right > 0 -> {
                    body += DiffLine(old++, new++, content)
                    left--
                    right--
                }
                else -> break
            }
            i++
        }
    }
    return Patch(path, hunks)
}

internal data class Piece(
    val call: String,
    val created: Boolean,
    val patch: String?,
    val diff: Patch?,
    val content: String?,
    val omitted: String?,
)

internal data class ReviewFile(val path: String, val pieces: List<Piece>)

internal data class Review(
    val files: List<ReviewFile>,
    val turn: String?,
    val latestChangedTurn: String?,
) {
    val reference: String
        get() {
            val scope = if (turn == null) "このセッションの変更" else "このターンの変更"
            val file =
                files
                    .firstOrNull()
                    ?.let {
                        "（${it.path}" +
                            (if (files.size > 1) " ほか ${files.size - 1} ファイル" else "") +
                            "）"
                    }
                    .orEmpty()
            return "$scope${file}について："
        }
}

private fun pathKey(path: String, cwd: String): String {
    if (path == "~" || path.startsWith("~/")) return path
    val full = if (!path.startsWith('/') && cwd.isNotEmpty()) "${cwd.trimEnd('/')}/$path" else path
    val parts = mutableListOf<String>()
    for (part in full.split('/')) when (part) {
        "",
        "." -> Unit
        ".." ->
            if (parts.isNotEmpty() && parts.last() != "..") parts.removeAt(parts.lastIndex)
            else if (!full.startsWith('/')) parts.add(part) else Unit
        else -> parts.add(part)
    }
    return (if (full.startsWith('/')) "/" else "./") + parts.joinToString("/")
}

internal fun resultPiece(name: String, result: JSONObject): Piece? {
    if (name !in listOf("write", "edit") || result.optBoolean("isError")) return null
    val details = result.optJSONObject("details")
    val write =
        details?.optJSONObject("pimate")?.takeIf {
            name == "write" && it.str("kind") == "write_diff"
        }
    val patch = (if (name == "edit") details else write)?.opt("patch") as? String
    val reason = if (result.optBoolean("detailsOmitted")) "too_large" else write?.str("omitted")
    return Piece(
        result.str("toolCallId"),
        write?.opt("existed") == false && reason != "concurrent",
        patch,
        patch?.let(::parsePatch),
        null,
        if (patch == null) reason?.ifEmpty { null } ?: "no_diff" else null,
    )
}

internal fun Piece.withWriteContent(content: String?): Piece =
    if (content == null || patch != null || omitted in listOf("binary", "too_large")) this
    else copy(content = content, omitted = if (omitted == "no_diff") "unknown_before" else omitted)

/** Scope calls by the initiating turn; a result can arrive in a later turn. */
internal fun review(entries: List<JSONObject>, cwd: String, turn: String? = null): Review {
    // Match the result to the most recent unmatched call, not every call sharing its ID.
    val pending = mutableMapOf<String, JSONObject>()
    val results = mutableMapOf<JSONObject, JSONObject>()
    for (entry in entries) when (entry.str("kind")) {
        "assistant" ->
            entry.optJSONArray("toolCalls").objects().forEach { pending[it.str("id")] = it }
        "tool_result" -> pending.remove(entry.str("toolCallId"))?.let { results[it] = entry }
    }
    val files = linkedMapOf<String, Pair<String, MutableList<Piece>>>()
    var latest: String? = null
    var current: String? = null
    for (entry in entries) {
        if (entry.str("kind") == "user") current = entry.str("entryId")
        if (entry.str("kind") != "assistant" || turn != null && turn != current) continue
        for (c in entry.optJSONArray("toolCalls").objects()) {
            val name = c.str("name")
            val result = results[c] ?: continue
            val args = c.optJSONObject("args")
            val piece =
                resultPiece(name, result)
                    ?.withWriteContent(
                        if (name == "write") args?.opt("content") as? String else null
                    ) ?: continue
            latest = current
            val path =
                (args
                        ?.str("path")
                        .orEmpty()
                        .ifEmpty { args?.str("file_path").orEmpty() }
                        .ifEmpty { piece.diff?.path ?: "（不明なファイル）" })
                    .removePrefix("@")
            files
                .getOrPut(pathKey(path, cwd)) {
                    (if (cwd.isEmpty()) path else path.removePrefix(cwd.trimEnd('/') + "/")) to
                        mutableListOf()
                }
                .second += piece
        }
    }
    return Review(files.values.map { ReviewFile(it.first, it.second) }, turn, latest)
}
