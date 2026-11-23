package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.json.*

/**
 * Pi からの質問はチャットの中に出す（モーダルにしない。docs/design/html/Chat-Waiting.html）。 確認の対象と本文は、回答のボタンより前に置く。書きかけの回答は
 * AnswerEdit（ViewModel 側）が持つ。
 */
@Composable
internal fun QuestionCard(
    q: JSONObject,
    edit: AnswerEdit,
    enabled: Boolean,
    answer: (JSONObject) -> Unit,
) {
    val kind = q.getString("kind")
    val message = q.str("message")
    val questions = q.optJSONArray("questions") ?: JSONArray()
    var ready = enabled
    Column(
        Modifier.fillMaxWidth()
            .background(pi.surface, RoundedCornerShape(16.dp))
            .padding(start = 16.dp, top = 16.dp, end = 16.dp, bottom = 12.dp)
    ) {
        // 届いたことを読み上げでも知らせる。
        Row(
            Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Dot(pi.waiting, 7.dp)
            Text(
                when (kind) {
                    "select" -> "選択してください"
                    "confirm" -> "確認してください"
                    "input" -> "入力してください"
                    "editor" -> "編集してください"
                    "questionnaire" -> "質問に答えてください"
                    else -> "PC で回答してください"
                },
                Modifier.padding(start = 8.dp),
                pi.waitingText,
                style = Type.caption,
                fontWeight = FontWeight.SemiBold,
            )
        }
        Text(
            q.getString("title"),
            Modifier.padding(top = 8.dp).semantics { heading() },
            fontWeight = FontWeight.SemiBold,
        )
        if (kind != "editor" && message.isNotEmpty()) Text(message, Modifier.padding(top = 4.dp))
        when (kind) {
            "select" -> {
                val options = q.optJSONArray("options") ?: JSONArray()
                Column(Modifier.padding(top = 4.dp).selectableGroup()) {
                    for (i in 0 until options.length()) {
                        val label = options.getString(i)
                        Option(label, "", edit.option == label, false, enabled) {
                            edit.option = label
                        }
                    }
                }
                ready = ready && edit.option != null
            }
            "input",
            "editor" -> {
                Field(
                    edit.value,
                    { edit.value = it },
                    q.str("placeholder").ifEmpty { "回答を入力" },
                    enabled,
                    single = kind == "input",
                    Modifier.padding(top = 10.dp),
                )
                if (q.optBoolean("tuiStaysOpen"))
                    Text(
                        "回答しても PC のエディタは開いたままです。PC 側で閉じてください。",
                        Modifier.padding(top = 8.dp),
                        pi.textSecondary,
                        style = Type.caption,
                    )
            }
            "questionnaire" ->
                for (i in 0 until questions.length()) {
                    val item = questions.getJSONObject(i)
                    val multi = item.optBoolean("multiSelect")
                    val draft = edit.choices[i] ?: Choice()
                    val question = item.getString("question")
                    // 質問が 1 つで題と同じなら、同じ文を 2 回出さない。
                    if (questions.length() > 1 || question != q.getString("title"))
                        Text(
                            if (questions.length() > 1) "${i + 1}/${questions.length()}　$question"
                            else question,
                            Modifier.padding(top = if (i == 0) 10.dp else 18.dp),
                            fontWeight = FontWeight.SemiBold,
                        )
                    if (multi) Text("当てはまるものをすべて選択", color = pi.textSecondary, style = Type.caption)
                    val opts = item.getJSONArray("options")
                    Column(if (multi) Modifier else Modifier.selectableGroup()) {
                        for (j in 0 until opts.length()) {
                            val o = opts.getJSONObject(j)
                            val label = o.getString("label")
                            Option(
                                label,
                                o.str("description"),
                                label in draft.labels,
                                multi,
                                enabled,
                            ) {
                                edit.choices[i] =
                                    if (!multi) Choice(setOf(label))
                                    else
                                        draft.copy(
                                            labels =
                                                if (label in draft.labels) draft.labels - label
                                                else draft.labels + label
                                        )
                            }
                        }
                    }
                    Field(
                        draft.custom,
                        {
                            edit.choices[i] =
                                draft.copy(
                                    custom = it,
                                    labels =
                                        if (!multi && it.isNotEmpty()) emptySet() else draft.labels,
                                )
                        },
                        "その他（自由に入力）",
                        enabled,
                        single = true,
                        Modifier.padding(top = 6.dp),
                    )
                    ready =
                        ready && (multi || draft.labels.isNotEmpty() || draft.custom.isNotBlank())
                }
            "confirm" -> Unit
            // スマホで答えられない種類は、ボタンを出さずにそう伝える。
            else -> return@Column
        }
        Row(
            Modifier.fillMaxWidth().padding(top = 14.dp),
            Arrangement.spacedBy(4.dp, Alignment.End),
            Alignment.CenterVertically,
        ) {
            TextAction(
                if (kind == "confirm") "いいえ" else "キャンセル",
                {
                    answer(
                        if (kind == "confirm") JSONObject().put("confirmed", false)
                        else JSONObject().put("cancelled", true)
                    )
                },
                enabled = enabled,
                color = pi.textSecondary,
                weight = FontWeight.Normal,
            )
            PrimaryButton(
                if (kind == "confirm") "はい" else "回答する",
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
                height = 40.dp,
            )
        }
    }
}

/** 選択肢の行（52dp 以上）。単一選択は丸、複数選択は四角の印。 */
@Composable
private fun Option(
    label: String,
    description: String,
    selected: Boolean,
    multi: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val shape = if (multi) RoundedCornerShape(5.dp) else CircleShape
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = 52.dp)
            .then(
                if (multi) Modifier.toggleable(selected, enabled, Role.Checkbox) { onClick() }
                else Modifier.selectable(selected, enabled, Role.RadioButton, onClick)
            )
            .alpha(if (enabled) 1f else .5f)
            .padding(vertical = 10.dp),
        Arrangement.spacedBy(12.dp),
    ) {
        Box(
            Modifier.padding(top = 1.5.dp)
                .size(18.dp)
                .then(
                    when {
                        selected && multi -> Modifier.background(pi.accent, shape)
                        selected -> Modifier.border(5.dp, pi.accent, shape)
                        else -> Modifier.border(1.5.dp, pi.textTertiary, shape)
                    }
                ),
            Alignment.Center,
        ) {
            if (selected && multi) Icon(Icons.Check, null, Modifier.size(13.dp), pi.onAccent)
        }
        Column {
            Text(label)
            if (description.isNotEmpty())
                Text(description, color = pi.textSecondary, style = Type.caption)
        }
    }
}

/** カードの中の入力欄。1 行はカプセル型、複数行は角丸 16。 */
@Composable
private fun Field(
    value: String,
    onChange: (String) -> Unit,
    placeholder: String,
    enabled: Boolean,
    single: Boolean,
    modifier: Modifier = Modifier,
) {
    BasicTextField(
        value,
        onChange,
        modifier
            .fillMaxWidth()
            .background(pi.background, if (single) CircleShape else RoundedCornerShape(16.dp))
            // 長い本文でも、回答のボタンまで送れる高さで止める（欄の中で送る）。
            .heightIn(min = if (single) tap else 96.dp, max = if (single) Dp.Unspecified else 260.dp)
            .padding(horizontal = 14.dp, vertical = if (single) 0.dp else 12.dp),
        enabled = enabled,
        textStyle = Type.body.copy(color = pi.text),
        singleLine = single,
        cursorBrush = SolidColor(pi.accent),
        decorationBox = { inner ->
            Box(contentAlignment = if (single) Alignment.CenterStart else Alignment.TopStart) {
                if (value.isEmpty()) Text(placeholder, color = pi.textSecondary)
                inner()
            }
        },
    )
}
