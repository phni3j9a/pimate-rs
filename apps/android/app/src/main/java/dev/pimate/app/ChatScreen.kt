package dev.pimate.app

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.json.*
import uniffi.pimate_ffi.*

@Composable
fun ColumnScope.ChatScreen(m: SessionModel) {
    val groups = m.rows.groups
    val entries = groups["entry"].orEmpty()
    val stream =
        groups["stream"]
            .orEmpty()
            .firstOrNull { it.id == "text" }
            ?.let { JSONTokener(it.json).nextValue() as? String }
            .orEmpty()
    val view = groups["view"].orEmpty().associate { it.id to it.json }
    val session =
        view["session"]?.let { (JSONTokener(it).nextValue() as? JSONObject)?.optString("id") }
    val target = m.selected?.let { if (session == null) null else Target(it, session) }
    LazyColumn(
        Modifier.weight(1f),
        reverseLayout = true,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(groups["question"].orEmpty()) {
            val question = JSONObject(it.json)
            Text(question.optString("title"))
            if (question.optString("kind") == "confirm" && target != null)
                Row {
                    listOf(true to "はい", false to "いいえ").forEach { (yes, label) ->
                        TextButton(
                            {
                                m.operate(
                                    Operation.Answer(
                                        target,
                                        JSONObject()
                                            .put("requestId", question.getString("requestId"))
                                            .put("confirmed", yes)
                                            .toString(),
                                    )
                                )
                            },
                            enabled = !m.busy,
                        ) {
                            Text(label)
                        }
                    }
                }
        }
        if (stream.isNotEmpty()) item("stream") { Markdown(stream) }
        items(entries.asReversed(), key = { "entry:${it.id}" }) {
            val obj = JSONObject(it.json)
            Markdown(obj.optString("text", obj.optString("kind")))
        }
    }
    var input by remember(target) { mutableStateOf("") }
    OutlinedTextField(
        input,
        { input = it },
        Modifier.fillMaxWidth(),
        label = { Text("メッセージ") },
        maxLines = 5,
    )
    Row {
        Button(
            {
                val text = input
                target?.let {
                    m.operate(Operation.Send(it, text, emptyList())) {
                        if (text == input) input = ""
                    }
                }
            },
            enabled = !m.busy && input.isNotBlank() && target != null,
        ) {
            Text(if (view["agentActive"] == "true") "割り込む" else "送信")
        }
        if (view["agentActive"] == "true" && target != null)
            TextButton({ m.operate(Operation.Abort(target)) }) { Text("停止") }
    }
}
