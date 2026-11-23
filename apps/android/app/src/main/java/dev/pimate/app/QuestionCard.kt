package dev.pimate.app

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import org.json.*

@Composable
internal fun QuestionCard(
    q: JSONObject,
    edit: AnswerEdit,
    enabled: Boolean,
    answer: (JSONObject) -> Unit,
) {
    val kind = q.getString("kind")
    val message = q.optString("message")
    val questions = q.optJSONArray("questions") ?: JSONArray()
    var ready = enabled
    Column {
        Text(
            when (kind) {
                "select" -> "選択してください"
                "confirm" -> "確認してください"
                "input" -> "入力してください"
                "editor" -> "編集してください"
                "questionnaire" -> "質問に答えてください"
                else -> "PC で回答してください"
            }
        )
        Text(q.getString("title"))
        if (kind != "editor" && message.isNotEmpty()) Text(message)
        when (kind) {
            "select" -> {
                val options = q.optJSONArray("options") ?: JSONArray()
                for (i in 0 until options.length()) {
                    val label = options.getString(i)
                    FilterChip(
                        edit.option == label,
                        { edit.option = label },
                        { Text(label) },
                        enabled = enabled,
                    )
                }
                ready = ready && edit.option != null
            }
            "input",
            "editor" -> {
                OutlinedTextField(
                    edit.value,
                    { edit.value = it },
                    enabled = enabled,
                    placeholder = { Text(q.optString("placeholder", "回答を入力")) },
                    singleLine = kind == "input",
                )
                if (q.optBoolean("tuiStaysOpen")) Text("回答しても PC のエディタは開いたままです。PC 側で閉じてください。")
            }
            "questionnaire" ->
                for (i in 0 until questions.length()) {
                    val item = questions.getJSONObject(i)
                    val multi = item.optBoolean("multiSelect")
                    val draft = edit.choices[i] ?: Choice()
                    Text("${i+1}/${questions.length()}　${item.getString("question")}")
                    if (multi) Text("当てはまるものをすべて選択")
                    val opts = item.getJSONArray("options")
                    for (j in 0 until opts.length()) {
                        val o = opts.getJSONObject(j)
                        val label = o.getString("label")
                        FilterChip(
                            label in draft.labels,
                            {
                                edit.choices[i] =
                                    if (!multi) Choice(setOf(label))
                                    else
                                        draft.copy(
                                            labels =
                                                if (label in draft.labels) draft.labels - label
                                                else draft.labels + label
                                        )
                            },
                            {
                                Column {
                                    Text(label)
                                    if (o.has("description")) Text(o.getString("description"))
                                }
                            },
                            enabled = enabled,
                        )
                    }
                    OutlinedTextField(
                        draft.custom,
                        {
                            edit.choices[i] =
                                draft.copy(
                                    custom = it,
                                    labels =
                                        if (!multi && it.isNotEmpty()) emptySet() else draft.labels,
                                )
                        },
                        enabled = enabled,
                        label = { Text("自分で入力") },
                    )
                    ready =
                        ready && (multi || draft.labels.isNotEmpty() || draft.custom.isNotBlank())
                }
            "confirm" -> Unit
            else -> return@Column
        }
        Row {
            TextButton(
                {
                    answer(
                        if (kind == "confirm") JSONObject().put("confirmed", false)
                        else JSONObject().put("cancelled", true)
                    )
                },
                enabled = enabled,
            ) {
                Text(if (kind == "confirm") "いいえ" else "キャンセル")
            }
            Button(
                {
                    val result = JSONObject()
                    when (kind) {
                        "confirm" -> result.put("confirmed", true)
                        "select" -> result.put("value", edit.option)
                        "questionnaire" -> {
                            val entries = JSONArray()
                            for (i in 0 until questions.length()) {
                                val d = edit.choices[i] ?: Choice()
                                entries.put(
                                    JSONObject()
                                        .put("selected", JSONArray(d.labels.toList()))
                                        .apply {
                                            if (d.custom.isNotBlank())
                                                put("custom", d.custom.trim())
                                        }
                                )
                            }
                            result.put("answers", entries)
                        }
                        else -> result.put("value", edit.value)
                    }
                    answer(result)
                },
                enabled = ready,
            ) {
                Text(if (kind == "confirm") "はい" else "回答する")
            }
        }
    }
}
