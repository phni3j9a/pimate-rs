package dev.pimate.app

import org.json.JSONObject
import org.json.JSONTokener
import uniffi.pimate_ffi.Change
import uniffi.pimate_ffi.Item

// Host ごとに実行中の要求・渡す結果・直近の移動を各1件。本文・下書きは保持しない。
internal data class PendingChatOp(
    val id: String,
    val instance: String,
    val source: String,
    val op: String,
    val arrival: String,
)

internal class Receipt(val request: String, private var owner: String?) {
    fun take(id: String, instance: String?): Boolean {
        if (instance == null || owner != instance || request != id) return false
        owner = null
        return true
    }
}

internal class ChatMove(val byPhone: Boolean)

internal data class ChatNotices(
    val pending: PendingChatOp? = null,
    val receipt: Receipt? = null,
    val move: ChatMove? = null,
) {
    /** 画面が再描画される前に busy が消える場合も、状態差分の順で開始・結果を照合する。 */
    fun advance(old: Rows, next: Rows, changes: List<Change>?): ChatNotices {
        var instance = old.instance()
        var pending = pending
        var receipt = receipt
        var move = move
        fun update(group: String, id: String, item: Item?) {
            when {
                group == "view" && id == "instanceId" ->
                    instance = item?.let { JSONTokener(it.json).nextValue() as? String }
                group == "busy" && id == "operation" -> {
                    pending =
                        item?.let {
                            val o = JSONObject(it.json)
                            val owner =
                                pending?.takeIf { p -> p.id == o.str("id") }?.instance ?: instance
                            if (
                                owner == null || o.str("method") !in listOf("session_op", "compact")
                            )
                                null
                            else
                                PendingChatOp(
                                    o.str("id"),
                                    owner,
                                    o.str("sessionId"),
                                    o.str("op"),
                                    o.str("arrival"),
                                )
                        }
                }
                group == "operationResult" && id == "latest" -> {
                    val resultId = item?.let { JSONObject(it.json).str("id") }
                    receipt =
                        receipt?.takeIf { it.request == resultId }
                            ?: pending
                                ?.takeIf { it.id == resultId }
                                ?.let { Receipt(it.id, it.instance) }
                }
                group == "notice" && id == "sessionChanged" ->
                    move =
                        item?.let {
                            val n = JSONObject(it.json)
                            val reason = n.str("reason")
                            val local =
                                pending?.let { p ->
                                    p.arrival.isNotEmpty() &&
                                        p.arrival == n.optJSONObject("to")?.str("id") &&
                                        p.source == n.optJSONObject("from")?.str("id") &&
                                        p.instance == instance &&
                                        (p.op == reason || p.op == "switch" && reason == "resume")
                                } == true
                            ChatMove(local)
                        }
            }
        }
        if (changes != null)
            for (change in changes) when (change) {
                is Change.Upsert -> update(change.v1.collection, change.v1.id, change.v1)
                is Change.Remove -> update(change.collection, change.id, null)
                else -> Unit
            }
        else {
            // 全量同期にイベント順はない。観測済みの要求にだけ結果を結び付ける。
            fun snap(group: String, id: String) =
                update(group, id, next.groups[group]?.find { it.id == id })
            snap("operationResult", "latest")
            snap("view", "instanceId")
            snap("busy", "operation")
            val notice = next.groups["notice"]?.find { it.id == "sessionChanged" }
            if (notice == null) move = null
            else if (old.revision != next.revision || old.groups["notice"]?.find { it.id == "sessionChanged" } != notice)
                move = ChatMove(false) // 未観測の移動で以前の確認・操作元を再利用しない。
        }
        return ChatNotices(pending, receipt, move)
    }
}

private fun Rows.instance() =
    groups["view"]
        ?.find { it.id == "instanceId" }
        ?.let { JSONTokener(it.json).nextValue() as? String }
