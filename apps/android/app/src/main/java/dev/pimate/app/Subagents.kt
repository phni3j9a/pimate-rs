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
