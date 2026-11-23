package dev.pimate.app

import androidx.compose.runtime.*
import org.json.JSONObject
import uniffi.pimate_ffi.RestoredDraft
import uniffi.pimate_ffi.Target

internal data class EditorKey(val host: String, val target: Target)

// UI-thread only. Neither composable lifetime nor SavedState owns private edits.
internal class ChatEdits {
    var current by mutableStateOf<EditorKey?>(null)
    private val bodies = mutableStateMapOf<EditorKey, RestoredDraft>()
    val body
        get() = bodies[current] ?: RestoredDraft("", emptyList())

    fun update(key: EditorKey, value: RestoredDraft) {
        if (key != current) return
        if (value.text.isEmpty() && value.images.isEmpty()) bodies.remove(key)
        else bodies[key] = value
    }

    fun text(key: EditorKey, text: String) {
        if (key == current) update(key, body.copy(text = text))
    }

    fun accepted(key: EditorKey, sent: RestoredDraft) {
        if (bodies[key] == sent) bodies.remove(key)
    }
}

internal data class Choice(val labels: Set<String> = emptySet(), val custom: String = "")

internal class AnswerEdit(initial: String) {
    var value by mutableStateOf(initial)
    var option by mutableStateOf<String?>(null)
    val choices = mutableStateMapOf<Int, Choice>()
}

internal class AnswerEdits {
    private val edits = mutableMapOf<Pair<EditorKey, String>, AnswerEdit>()

    fun sync(key: EditorKey, pending: List<uniffi.pimate_ffi.Item>) {
        val ids = pending.mapTo(mutableSetOf()) { it.id }
        edits.keys.removeAll { it.first == key && it.second !in ids }
        pending.forEach { item ->
            edits.getOrPut(key to item.id) {
                val q = JSONObject(item.json)
                AnswerEdit(if (q.optString("kind") == "editor") q.optString("message") else "")
            }
        }
    }

    operator fun get(key: EditorKey, id: String) = edits[key to id]
}
