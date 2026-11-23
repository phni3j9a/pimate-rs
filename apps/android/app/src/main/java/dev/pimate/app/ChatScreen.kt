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
    val target = m.target
    val editor = m.edits.current
    val body = m.edits.body
    val scroll = rememberLazyListState()
    val questions = groups["question"].orEmpty()
    LaunchedEffect(questions.firstOrNull()?.id) {
        if (questions.isNotEmpty()) scroll.scrollToItem(0)
    }
    LazyColumn(
        Modifier.weight(1f),
        state = scroll,
        reverseLayout = true,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(questions, key = { "question:${it.id}" }) { item ->
            key(m.host?.id, target, item.json) {
                val edit = editor?.let { m.answers[it, item.id] }
                if (edit != null)
                    QuestionCard(
                        remember(item.json) { JSONObject(item.json) },
                        edit,
                        enabled =
                            target?.let {
                                m.can(
                                    Operation.Answer(
                                        it,
                                        JSONObject()
                                            .put("requestId", item.id)
                                            .put("cancelled", true)
                                            .toString(),
                                    )
                                )
                            } == true,
                    ) { answer ->
                        m.answer(editor, item.id, edit, answer)
                    }
            }
        }
        if (stream.isNotEmpty()) item("stream") { Markdown(stream) }
        items(entries.asReversed(), key = { "entry:${it.id}" }) {
            val obj = JSONObject(it.json)
            Markdown(obj.optString("text", obj.optString("kind")))
        }
    }
    LazyColumn(Modifier.heightIn(max = 160.dp)) {
        items(groups["draft"].orEmpty(), key = { "draft:${it.id}" }) { draft ->
            val d = remember(draft.json) { JSONObject(draft.json) }
            val status = d.optString("status")
            if (
                d.optString("instanceId") == m.selected &&
                    status in listOf("queued", "unconfirmed", "sending")
            )
                Column {
                    Text(
                        when (status) {
                            "queued" -> "割り込み待ち"
                            "sending" -> "送信中"
                            else -> "届いていない可能性があります。履歴を確認してください。"
                        }
                    )
                    Text(d.getJSONObject("params").optString("text"))
                    if (status == "unconfirmed")
                        TextButton(
                            { editor?.let { m.restore(it, draft.id) } },
                            enabled =
                                !m.busy &&
                                    body.text.isEmpty() &&
                                    body.images.isEmpty() &&
                                    target != null &&
                                    target.session == (d.opt("sessionId") as? String),
                        ) {
                            Text("下書きへ戻す")
                        }
                }
        }
    }
    key(editor) {
        if (body.images.isNotEmpty()) Text("添付画像 ${body.images.size} 枚")
        OutlinedTextField(
            body.text,
            { text -> editor?.let { m.edits.text(it, text) } },
            Modifier.fillMaxWidth(),
            label = { Text("メッセージ") },
            enabled = target != null,
            maxLines = 5,
        )
    }
    Row {
        Button(
            { editor?.let { m.send(it, body) } },
            enabled =
                (body.text.isNotBlank() || body.images.isNotEmpty()) &&
                    m.can(target?.let { Operation.Send(it, body.text, body.images) }),
        ) {
            Text(if (view["agentActive"] == "true") "割り込む" else "送信")
        }
        if (view["agentActive"] == "true" && target != null)
            TextButton(
                { m.operate(Operation.Abort(target)) },
                enabled = m.can(Operation.Abort(target)),
            ) {
                Text("停止")
            }
    }
}
