package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.json.*
import uniffi.pimate_ffi.CoreException
import uniffi.pimate_ffi.RcMethod

/**
 * セッションツリー（PC の /tree）。現在の枝を左の線の色で示し、メッセージを選ぶと、そこへの移動とラベルの編集ができる。 移動が受け付けられたら閉じ、結果は会話の欄で示す。
 */
@Composable
internal fun TreeScreen(m: SessionModel, key: EditorKey, close: () -> Unit) {
    val ops = rememberOps(m, key)
    var tree by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf("") }
    var load by remember { mutableIntStateOf(0) }
    var loading by remember { mutableStateOf(true) }
    var usersOnly by rememberSaveable { mutableStateOf(true) }
    var selected by rememberSaveable { mutableStateOf<String?>(null) }
    LaunchedEffect(key, load) {
        loading = true
        try {
            tree = JSONObject(m.ask(key, RcMethod.SESSION_TREE, JSONObject()))
            error = ""
        } catch (e: CoreException) {
            error = failure(e)
        } finally {
            loading = false
        }
    }
    val rows =
        remember(tree, usersOnly) { tree?.optJSONArray("nodes")?.let { treeRows(it, usersOnly) }.orEmpty() }
    val view = m.rows.groups["view"].orEmpty().associate { it.id to it.json }
    val idle = view["status"] == "\"idle\"" && view["agentActive"] != "true"
    val now = remember(tree) { System.currentTimeMillis() }
    FullScreen(close) {
        Column(Modifier.fillMaxSize().background(pi.background).safeDrawingPadding()) {
            Row(
                Modifier.height(52.dp).padding(start = 2.dp, end = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconAction(Icons.Back, "チャットに戻る", close, tint = pi.text)
                Text(
                    "セッションツリー",
                    Modifier.weight(1f).semantics { heading() },
                    style = Type.header,
                )
                TextAction("更新", { load++ }, enabled = ops.live && !loading)
            }
            Segment(
                listOf("users" to "ユーザーのみ", "all" to "すべて"),
                if (usersOnly) "users" else "all",
                Modifier.padding(horizontal = gutter),
            ) {
                usersOnly = it == "users"
            }
            Text(
                when {
                    !ops.live -> "会話が変わりました。チャットからツリーを開き直してください。"
                    error.isNotEmpty() -> error
                    tree?.optBoolean("truncated") == true ->
                        "大きなツリーのため一部を省略しています（最大 1,000 件）。全体は PC の /tree で確認できます。"
                    else -> "現在の枝は青い線で示します。メッセージを選ぶと、移動とラベルの編集ができます。"
                },
                Modifier.padding(horizontal = gutter, vertical = 12.dp).semantics {
                    liveRegion = LiveRegionMode.Polite
                },
                if (!ops.live || error.isNotEmpty()) pi.failedText else pi.textSecondary,
                style = Type.caption,
            )
            if (loading && tree == null)
                Row(
                    Modifier.fillMaxWidth().padding(12.dp),
                    Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                    Alignment.CenterVertically,
                ) {
                    Spinner()
                    Text("読み込み中…", color = pi.textSecondary, style = Type.caption)
                }
            else if (rows.isEmpty() && error.isEmpty())
                Text(
                    if (usersOnly) "ユーザーのメッセージはありません" else "まだ会話がありません",
                    Modifier.fillMaxWidth().padding(24.dp),
                    pi.textSecondary,
                    style =
                        Type.caption.copy(textAlign = androidx.compose.ui.text.style.TextAlign.Center),
                )
            LazyColumn(
                Modifier.weight(1f),
                contentPadding = PaddingValues(start = gutter, end = gutter, bottom = 20.dp),
            ) {
                items(if (ops.live) rows else emptyList(), key = { it.id }) { row ->
                    val line = if (row.onBranch) pi.accent else pi.surfaceStrong
                    Row(
                        Modifier.padding(start = (minOf(row.depth, 6) * 14).dp)
                            .fillMaxWidth()
                            .heightIn(min = 64.dp)
                            .clip(RoundedCornerShape(topEnd = 12.dp, bottomEnd = 12.dp))
                            .clickable(role = Role.Button) { selected = row.id }
                            .drawBehind {
                                drawRoundRect(
                                    line,
                                    size = Size(2.dp.toPx(), size.height),
                                    cornerRadius = CornerRadius(1.dp.toPx()),
                                )
                            }
                            .semantics { if (row.current) stateDescription = "現在位置" }
                            .padding(start = 12.dp, top = 10.dp, bottom = 10.dp),
                        Arrangement.spacedBy(8.dp),
                        Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f), Arrangement.spacedBy(2.dp)) {
                            Text(
                                listOfNotNull(
                                        treeKind(row.kind),
                                        row.time?.let { agoShort(it, now) },
                                        when {
                                            row.current -> "現在位置"
                                            row.leaf -> "枝の先端"
                                            else -> null
                                        },
                                    )
                                    .joinToString(" · "),
                                color = pi.textSecondary,
                                style = Type.caption,
                            )
                            if (row.label.isNotEmpty())
                                Text(
                                    row.label,
                                    fontWeight = FontWeight.SemiBold,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            Text(
                                row.preview.ifEmpty { "（本文なし）" },
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        if (row.current) Icon(Icons.Check, null, Modifier.size(16.dp), pi.accent)
                    }
                }
            }
        }
        rows
            .find { it.id == selected }
            ?.let { row ->
                NodeSheet(row, ops, idle, { load++ }, { selected = null }) {
                    selected = null
                    close()
                }
            }
    }
}

private fun agoShort(at: Long, now: Long) =
    ago(at, now).let { if (it != "昨日" && it.last() in "分間日") "${it}前" else it }

/** 選んだメッセージの操作: ラベルと、そこへの移動。 */
@Composable
private fun NodeSheet(
    row: TreeRow,
    ops: Ops,
    idle: Boolean,
    reload: () -> Unit,
    dismiss: () -> Unit,
    moved: () -> Unit,
) {
    var label by rememberSaveable(row.id) { mutableStateOf(row.label) }
    var summarize by rememberSaveable(row.id) { mutableStateOf(false) }
    fun save(value: String) =
        ops.run(
            "label",
            RcMethod.SET_LABEL,
            JSONObject().put("entryId", row.id).apply {
                if (value.isNotBlank()) put("label", value.trim())
            },
        ) {
            label = value.trim()
            reload()
        }
    PiSheet(dismiss) {
        SheetTitle("メッセージの操作")
        Text(
            row.preview.ifEmpty { "（本文なし）" },
            Modifier.padding(top = 8.dp),
            pi.textSecondary,
            maxLines = 3,
            overflow = TextOverflow.Ellipsis,
        )
        if (ops.error.isNotEmpty())
            Text(
                ops.error,
                Modifier.padding(top = 10.dp).semantics { liveRegion = LiveRegionMode.Polite },
                pi.failedText,
                style = Type.caption,
            )
        Text(
            "ラベル",
            Modifier.padding(top = 20.dp, bottom = 8.dp).semantics { heading() },
            fontWeight = FontWeight.SemiBold,
        )
        BasicTextField(
            label,
            { label = it.take(200) },
            Modifier.fillMaxWidth()
                .heightIn(min = tap)
                .background(pi.surface, CircleShape)
                .padding(horizontal = 16.dp)
                .semantics { contentDescription = "ラベル" },
            enabled = ops.ready,
            textStyle = Type.body.copy(color = pi.text),
            singleLine = true,
            cursorBrush = SolidColor(pi.accent),
            decorationBox = { inner ->
                Box(contentAlignment = Alignment.CenterStart) {
                    if (label.isEmpty()) Text("ラベルを入力", color = pi.textSecondary)
                    inner()
                }
            },
        )
        Row(Modifier.offset(x = (-14).dp)) {
            TextAction(
                if (ops.busy == "label") "保存中…" else "ラベルを保存",
                { save(label) },
                enabled = ops.ready,
            )
            if (row.label.isNotEmpty())
                TextAction("ラベルを外す", { save("") }, enabled = ops.ready, color = pi.textSecondary)
        }
        Text(
            "ここへ移動",
            Modifier.padding(top = 12.dp, bottom = 8.dp).semantics { heading() },
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            "ユーザーのメッセージへ移動すると、その直前まで戻ります。入力欄が空なら、選んだ文が入ります。元の枝は残ります。",
            color = pi.textSecondary,
            style = Type.caption,
        )
        Row(
            Modifier.fillMaxWidth()
                .heightIn(min = 52.dp)
                .toggleable(summarize, ops.ready, Role.Switch) { summarize = it },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("離れる枝を要約する", Modifier.weight(1f))
            Switch(
                summarize,
                null,
                enabled = ops.ready,
                colors =
                    SwitchDefaults.colors(
                        checkedTrackColor = pi.accent,
                        checkedThumbColor = pi.onAccent,
                        uncheckedTrackColor = pi.surfaceStrong,
                        uncheckedThumbColor = pi.background,
                        uncheckedBorderColor = pi.surfaceStrong,
                    ),
            )
        }
        Text(
            if (idle) "要約には LLM を使うため、時間と利用料金がかかります。"
            else "実行中は移動できません。Pi の処理が終わるまでお待ちください。",
            color = pi.textSecondary,
            style = Type.caption,
        )
        PrimaryButton(
            if (ops.busy == "tree") "移動しています…" else "ここへ移動",
            {
                ops.run(
                    "tree",
                    RcMethod.SESSION_OP,
                    JSONObject().put("op", "tree").put("arg", row.id).put("summarize", summarize),
                ) {
                    moved()
                }
            },
            Modifier.padding(top = 16.dp).fillMaxWidth(),
            enabled = ops.ready && idle,
        )
    }
}
