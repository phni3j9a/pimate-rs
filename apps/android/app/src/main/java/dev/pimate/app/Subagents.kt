package dev.pimate.app

import androidx.compose.runtime.*
import org.json.JSONObject
import uniffi.pimate_ffi.RcMethod

internal fun Rows.subagentCounts(): Map<String, Int> {
    if (groups["runWatch"].isNullOrEmpty()) return emptyMap()
    return groups["run"]
        .orEmpty()
        .flatMap { row ->
            val run = JSONObject(row.json)
            val children = run.optJSONArray("children").objects().ifEmpty { listOf(run) }
            val started = children.any { it.str("state") != "queued" }
            children.mapNotNull { child ->
                when (val state = child.str("state")) {
                    "queued" -> if (started) null else "running"
                    "running",
                    "stopping" -> "running"
                    "complete",
                    "failed",
                    "waiting",
                    "paused",
                    "stopped" -> state
                    else -> null
                }
            }
        }
        .groupingBy { it }
        .eachCount()
}

/** One screen/child owns this loader. Invoke from its main-thread coroutine scope. */
internal class WorkRecord(private val page: suspend (String?) -> JSONObject) {
    var items by mutableStateOf(emptyList<JSONObject>())
        private set

    var task by mutableStateOf("")
        private set

    var truncated by mutableStateOf(false)
        private set

    var more by mutableStateOf(false)
        private set

    private var cursor: String? = null
    private var loading = false
    private var again = false

    suspend fun load(): Boolean {
        if (loading) {
            again = true
            return false
        }
        loading = true
        again = false
        try {
            for (i in 1..8) {
                val value = page(cursor)
                val next = value.str("cursor").ifEmpty { null }
                items =
                    ((if (cursor == null) emptyList() else items) +
                            value.optJSONArray("items").objects())
                        .associateBy { it.getString("entryId") }
                        .values
                        .toList()
                truncated = truncated || value.optBoolean("truncated")
                if (!value.isNull("task")) task = value.str("task")
                more = next != null && next != cursor
                cursor = next ?: cursor
                if (!more) break
            }
        } finally {
            loading = false
        }
        return again
    }
}

internal fun SessionModel.workRecord(key: EditorKey, run: String, child: String) =
    WorkRecord { cursor ->
        JSONObject(
            request(
                key,
                RcMethod.SUBAGENT_TRANSCRIPT,
                JSONObject().put("runId", run).put("childId", child).put("cursor", cursor),
            )
        )
    }

internal data class ChildKey(val editor: EditorKey, val run: String, val child: String)

internal data class SentInstruction(val action: String, val text: String)

internal class ChildInstructions {
    private val sent = mutableStateMapOf<ChildKey, List<SentInstruction>>()

    fun accepted(key: ChildKey, action: String, text: String) {
        sent[key] = sent[key].orEmpty() + SentInstruction(action, text)
    }

    fun forChild(key: ChildKey, resumedFrom: String?) =
        sent[key].orEmpty() +
            resumedFrom?.takeUnless { it == key.run }?.let { sent[key.copy(run = it)] }.orEmpty()

    fun forget(host: String) {
        sent.keys.filter { it.editor.host == host }.forEach(sent::remove)
    }
}

internal suspend fun SessionModel.control(
    key: EditorKey,
    run: String,
    child: String?,
    action: String,
    text: String? = null,
): JSONObject {
    val body = text?.trim()
    require(action !in listOf("resume", "steer", "follow_up") || !body.isNullOrEmpty())
    val result =
        request(
            key,
            RcMethod.SUBAGENT_CONTROL,
            JSONObject()
                .put("runId", run)
                .put("childId", child)
                .put("action", action)
                .put("text", body),
        )
    if (child != null && !body.isNullOrEmpty())
        instructions.accepted(ChildKey(key, run, child), action, body)
    return JSONObject(result)
}
