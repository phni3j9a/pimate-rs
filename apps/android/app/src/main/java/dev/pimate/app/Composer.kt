package dev.pimate.app

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts.PickMultipleVisualMedia
import androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.*
import uniffi.pimate_ffi.*

private enum class Tone {
    Normal,
    Waiting,
    Failed,
}

/** 入力欄の中の案内（Chat-Edge）。印・題・説明と、右に操作か「閉じる」。 */
@Composable
private fun Banner(
    title: String,
    text: String = "",
    tone: Tone = Tone.Normal,
    icon: ImageVector? = null,
    action: String? = null,
    onAction: () -> Unit = {},
    dismiss: (() -> Unit)? = null,
) {
    val color =
        when (tone) {
            Tone.Normal -> pi.textSecondary
            Tone.Waiting -> pi.waitingText
            Tone.Failed -> pi.failedText
        }
    Row(
        Modifier.fillMaxWidth().padding(bottom = 8.dp).semantics(mergeDescendants = true) {
            liveRegion = LiveRegionMode.Polite
        },
        Arrangement.spacedBy(8.dp),
    ) {
        Box(Modifier.padding(top = 2.dp).size(14.dp), Alignment.Center) {
            if (icon == null) Spinner() else Icon(icon, null, Modifier.size(14.dp), color)
        }
        Column(Modifier.weight(1f)) {
            if (title.isNotEmpty())
                Text(title, color = color, style = Type.caption, fontWeight = FontWeight.SemiBold)
            if (text.isNotEmpty()) Text(text, color = color, style = Type.caption)
        }
        if (action != null)
            Text(
                action,
                Modifier.clip(RoundedCornerShape(8.dp))
                    .clickable(role = Role.Button, onClick = onAction)
                    .padding(horizontal = 6.dp, vertical = 2.dp),
                pi.accentText,
                style = Type.body.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold),
            )
        if (dismiss != null)
            Box(
                Modifier.size(24.dp).clip(CircleShape).clickable(role = Role.Button, onClick = dismiss),
                Alignment.Center,
            ) {
                Icon(Icons.Close, "案内を閉じる", Modifier.size(14.dp), pi.textSecondary)
            }
    }
}

/** 実行中の操作（busy/operation）の呼び名。 */
private fun working(op: JSONObject) =
    when (if (op.str("method") == "session_op") op.str("op") else op.str("method")) {
        "compact" -> "コンテキストを圧縮しています…"
        "new" -> "新しいセッションを開いています…"
        "switch" -> "セッションを開いています…"
        "fork" -> "分岐しています…"
        "tree" -> "ツリーの位置へ移動しています…"
        "reload" -> "再読み込みしています…"
        "shutdown" -> "Pi を終了しています…"
        else -> ""
    }

/** 終わった操作（operationResult/latest）の案内。成功して会話に表れるもの（新規・切り替え・分岐）は null。 */
private fun finished(result: JSONObject): Triple<String, String, Tone>? {
    val event = result.optJSONObject("event")
    val op = if (event?.str("type") == "session_op_done") event.str("op") else "compact"
    val name =
        when (op) {
            "compact" -> "圧縮"
            "new" -> "新しいセッション"
            "switch" -> "セッションの切り替え"
            "fork" -> "分岐"
            "tree" -> "ツリーの移動"
            "reload" -> "再読み込み"
            else -> "操作"
        }
    return when {
        result.str("outcome") == "unknown" ->
            Triple("操作の結果を確認できませんでした", "もう一度試す前に、会話と PC の状態を確認してください。", Tone.Waiting)
        event == null -> null
        event.has("error") -> Triple("${name}できませんでした", event.str("error").take(200), Tone.Failed)
        event.optBoolean("cancelled") ->
            Triple("${name}は取り消されました", "Pi の拡張機能などが止めました。", Tone.Normal)
        op == "compact" -> Triple("コンテキストを圧縮しました", "", Tone.Normal)
        op == "tree" -> Triple("ツリーの位置へ移動しました", "", Tone.Normal)
        op == "reload" -> Triple("再読み込みしました", "", Tone.Normal)
        else -> null
    }
}

/** 入力欄と、その上の送信待ち・コマンドの候補。受け付けられただけの本文は履歴に混ぜず、ここに分けて出す。 */
@Composable
internal fun Composer(
    m: SessionModel,
    ui: ChatUi,
    /** 操作を送れる対象。読み込み中・切断中は null。 */
    key: EditorKey?,
    active: Boolean,
    asking: Boolean,
    pcPrompt: Boolean,
    offline: Boolean,
    /** 切り替え前の会話に下書きが残った（確認するまで送らない）。 */
    left: Boolean,
    /** 切り替えの前の会話と、いまの会話の名前。 */
    names: Pair<String, String>?,
    confirm: () -> Unit,
    busyOp: JSONObject?,
    /** まだ案内していない操作結果。 */
    result: JSONObject?,
    commandsFailed: Boolean,
    retryCommands: () -> Unit,
    show: (String) -> Unit,
) {
    val target = m.target
    val editor = m.edits.current
    val body = m.edits.body
    val groups = m.rows.groups
    val view = groups["view"].orEmpty().associate { it.id to it.json }
    val pics = LocalPics.current
    val scope = rememberCoroutineScope()
    val drafts =
        groups["draft"].orEmpty().mapNotNull { item ->
            val d = JSONObject(item.json)
            Triple(item.id, d, d.str("status")).takeIf {
                d.str("instanceId") == m.selected &&
                    it.third in listOf("queued", "unconfirmed", "sending")
            }
        }

    // ---- 添付（システムのフォトピッカー。対象は開いた瞬間の EditorKey） ----
    val limit =
        if (view["promptImages"] == "true") minOf(view["promptImageLimit"]?.toIntOrNull() ?: 1, 4)
        else 0
    val picked = { uris: List<Uri> ->
        val at = ui.picking
        ui.picking = null
        if (at != null && uris.isNotEmpty()) {
            ui.encoding = true
            // 縮小は回転をまたいで続ける（結果は ChatEdits に入る）。
            ui.viewModelScope.launch {
                try {
                    m.attach(at, uris)
                    if (m.edits.current != at)
                        ui.problem =
                            Triple("画像を添付しませんでした", "送信先が変わりました。もう一度選んでください。", true)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    // Attachments.kt は、利用者に見せる理由を日本語のメッセージで投げる。それ以外（読み取りの失敗など）は定型文。
                    ui.problem =
                        Triple(
                            "画像を添付できませんでした",
                            e.message?.takeIf { it.isNotEmpty() && it[0].code > 0x2E7F }
                                ?: "画像を読み込めませんでした。もう一度選んでください。",
                            true,
                        )
                } finally {
                    ui.encoding = false
                }
            }
        }
    }
    val one = rememberLauncherForActivityResult(PickVisualMedia()) { picked(listOfNotNull(it)) }
    // 選べる枚数は、あと何枚添付できるかに合わせる（上限は登録時に決まるので、2〜4 枚ぶんを用意する）。
    val many = (2..4).map { rememberLauncherForActivityResult(PickMultipleVisualMedia(it), picked) }
    val attach = {
        val room = limit - body.images.size
        when {
            key == null -> Unit
            limit == 0 ->
                ui.problem =
                    Triple(
                        "この Pi は画像の添付に対応していません",
                        "PC の PiMate を更新して、Pi を起動し直してください。文字は送れます。",
                        false,
                    )
            room <= 0 ->
                ui.problem = Triple("添付できる画像は $limit 枚までです", "送る前に、どれかを外してください。", false)
            else -> {
                ui.problem = null
                ui.picking = key
                (if (room == 1) one else many[room - 2]).launch(
                    PickVisualMediaRequest(PickVisualMedia.ImageOnly)
                )
            }
        }
    }

    // ---- / コマンド ----
    val slash = view["slashCommands"] == "true"
    val commands = groups["command"]
    val name = if (slash) slashName(body.text) else null
    val command =
        remember(name, commands) {
            name?.let { n -> commands.orEmpty().find { it.id == n }?.let { JSONObject(it.json) } }
        }
    val builtin = command?.str("source") == "builtin"
    val bare = name != null && body.text.trim() == "/$name" && body.images.isEmpty()
    val hint =
        when {
            name == null || key == null -> ""
            command == null ->
                if (body.text.any { it.isWhitespace() } && !commandsFailed)
                    "/$name は使えません。一覧から選んでください。"
                else ""
            builtin && !bare -> "アプリの操作には、引数や画像を付けずに送信してください。"
            command.str("source") == "extension" && body.images.isNotEmpty() ->
                "Extension のコマンドには画像を添付できません。"
            else -> ""
        }
    val run = { n: String ->
        if (key != null)
            when (n) {
                "model" -> show("model")
                "resume" -> show("sessions")
                "fork" -> show("fork")
                "tree" -> show("tree")
                else ->
                    scope.launch {
                        try {
                            if (n == "compact") m.ask(key, RcMethod.COMPACT, JSONObject())
                            else m.ask(key, RcMethod.SESSION_OP, JSONObject().put("op", n))
                            m.edits.text(key, "")
                        } catch (e: CoreException) {
                            ui.problem = Triple("/$n を実行できませんでした", failure(e), true)
                        }
                    }
            }
        if (n in listOf("model", "resume", "fork", "tree")) key?.let { m.edits.text(it, "") }
    }
    val canSend =
        when {
            left || key == null || ui.encoding -> false
            builtin -> bare && !m.busy && busyOp == null
            else ->
                (body.text.isNotBlank() || body.images.isNotEmpty()) &&
                    m.can(Operation.Send(key.target, body.text, body.images))
        }

    Column(Modifier.padding(start = 16.dp, top = 6.dp, end = 16.dp, bottom = 8.dp)) {
        if (drafts.isNotEmpty())
            Column(
                Modifier.heightIn(max = 200.dp)
                    .verticalScroll(rememberScrollState())
                    .padding(bottom = 6.dp)
            ) {
                drafts.forEach { (id, d, status) ->
                    val params = d.optJSONObject("params")
                    val text = params?.str("text").orEmpty()
                    val images = params?.optJSONArray("images")
                    val sent =
                        remember(id, images?.length()) {
                            List(images?.length() ?: 0) {
                                val image = images!!.getJSONObject(it)
                                Pic.Inline("", image.str("mimeType"), image.str("data"))
                            }
                        }
                    if (status == "unconfirmed") {
                        val restorable =
                            !m.busy &&
                                body.text.isEmpty() &&
                                body.images.isEmpty() &&
                                target != null &&
                                target.session == (d.opt("sessionId") as? String)
                        Column(
                            Modifier.fillMaxWidth()
                                .padding(bottom = 6.dp)
                                .background(pi.surface, RoundedCornerShape(16.dp))
                                .padding(start = 14.dp, top = 12.dp, end = 4.dp, bottom = 2.dp)
                        ) {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(Icons.Clock, null, Modifier.size(12.dp), pi.waitingText)
                                Text(
                                    if (offline) "届いたか未確認" else "届いていない可能性があります",
                                    color = pi.waitingText,
                                    style = Type.caption,
                                    fontWeight = FontWeight.SemiBold,
                                )
                            }
                            Text(
                                if (offline) "再接続したら確認します（自動で再送しません）。"
                                else "PC の入力欄に戻っている場合があります。履歴を確認してください。",
                                Modifier.padding(end = 10.dp),
                                pi.textSecondary,
                                style = Type.caption,
                            )
                            if (sent.isNotEmpty()) Thumbs(sent, Modifier.padding(top = 6.dp))
                            if (text.isNotEmpty())
                                Text(
                                    text,
                                    Modifier.padding(top = 6.dp, end = 10.dp),
                                    maxLines = 3,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            TextAction(
                                "下書きへ戻す",
                                { editor?.let { m.restore(it, id) } },
                                Modifier.offset(x = (-14).dp),
                                enabled = restorable,
                            )
                        }
                    } else
                        Row(
                            Modifier.fillMaxWidth()
                                .heightIn(min = 40.dp)
                                .padding(start = 12.dp, end = 12.dp)
                                .semantics(mergeDescendants = true) {},
                            Arrangement.spacedBy(8.dp),
                            Alignment.CenterVertically,
                        ) {
                            Icon(Icons.Reply, null, Modifier.size(14.dp), pi.textSecondary)
                            Text(
                                if (status == "queued") "割り込み待ち" else "送信中",
                                color = pi.textSecondary,
                                style = Type.caption,
                            )
                            Text(
                                listOfNotNull(
                                        "画像 ${sent.size} 枚".takeIf { sent.isNotEmpty() },
                                        text.takeIf { it.isNotEmpty() },
                                    )
                                    .joinToString(" · "),
                                Modifier.weight(1f),
                                style = Type.body.copy(fontSize = 13.sp),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                }
            }
        // 候補は入力欄のすぐ上。選ぶと入力欄に入るだけで、送信はしない。
        if (slash && key != null && Regex("/\\S*").matches(body.text))
            CommandPalette(m, body.text, commands, commandsFailed, retryCommands) {
                m.edits.text(key, "/$it ")
            }
        (m.error.ifEmpty { hint }).let {
            if (it.isNotEmpty())
                Text(
                    it,
                    Modifier.padding(start = 12.dp, end = 12.dp, bottom = 6.dp).semantics {
                        liveRegion = LiveRegionMode.Polite
                    },
                    if (m.error.isNotEmpty()) pi.failedText else pi.textSecondary,
                    style = Type.caption,
                )
        }
        Column(
            Modifier.background(pi.surface, RoundedCornerShape(24.dp))
                .padding(start = 14.dp, top = 12.dp, end = 8.dp, bottom = 6.dp)
        ) {
            if (left && names != null)
                Banner(
                    "送信先が変わりました",
                    "ここからは「${names.second}」に送られます。" +
                        "書きかけは「${names.first.ifEmpty { "前の会話" }}」に残しました",
                    Tone.Waiting,
                    Icons.Swap,
                    "確認する",
                    confirm,
                )
            busyOp?.let(::working)?.takeIf { it.isNotEmpty() }?.let { Banner(it) }
            result?.let { r ->
                finished(r)?.let { (title, text, tone) ->
                    Banner(
                        title,
                        text,
                        tone,
                        if (tone == Tone.Normal) Icons.Check else Icons.Clock,
                        dismiss = { ui.seenResult = r.str("id") },
                    )
                }
            }
            ui.problem?.let { (title, text, failed) ->
                Banner(
                    title,
                    text,
                    if (failed) Tone.Failed else Tone.Normal,
                    Icons.Close,
                    dismiss = { ui.problem = null },
                )
            }
            if (body.images.isNotEmpty() && editor != null) {
                val shown =
                    remember(body.images) {
                        body.images.map { Pic.Inline("", it.mimeType, it.data) }
                    }
                Thumbs(shown, Modifier.padding(bottom = 8.dp)) { index ->
                    m.edits.update(
                        editor,
                        body.copy(images = body.images.filterIndexed { i, _ -> i != index }),
                    )
                }
            }
            val waiting =
                when {
                    m.selected == null -> "Pi を選ぶと入力できます"
                    offline -> "つながると入力できます"
                    target == null -> "読み込み中…"
                    asking -> "上の質問に回答すると入力できます"
                    pcPrompt -> "PC での操作が終わると入力できます"
                    else -> "メッセージ"
                }
            // 入力は ViewModel（ChatEdits）が持つ。欄は編集の対象ごとに作り直し、古い IME の確定を捨てる。
            key(editor) {
                // ここで持つのはカーソルと変換中の範囲だけ。本文が外から変わったら（復元・候補・分岐）、末尾に置き直す。
                var field by remember {
                    mutableStateOf(TextFieldValue(body.text, TextRange(body.text.length)))
                }
                if (field.text != body.text)
                    field = TextFieldValue(body.text, TextRange(body.text.length))
                BasicTextField(
                    field,
                    { next ->
                        field = next
                        editor?.let { m.edits.text(it, next.text) }
                    },
                    Modifier.fillMaxWidth().padding(end = 6.dp).semantics {
                        contentDescription = "メッセージ"
                    },
                    enabled = target != null && !asking && !pcPrompt,
                    textStyle = Type.lead.copy(color = pi.text),
                    maxLines = 5,
                    cursorBrush = SolidColor(pi.accent),
                    decorationBox = { inner ->
                        if (body.text.isEmpty())
                            Text(
                                waiting,
                                color = pi.textSecondary,
                                style = Type.lead,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        inner()
                    },
                )
            }
            Row(
                Modifier.padding(top = 6.dp).heightIn(min = 36.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
              // 左の 3 つ（添付・モデル・使用状況）。狭い画面では、モデルの名前だけを縮める。
              Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier.offset(x = (-6).dp)
                        .size(36.dp)
                        .clip(CircleShape)
                        .clickable(key != null && !ui.encoding, role = Role.Button, onClick = attach)
                        .semantics {
                            contentDescription =
                                if (ui.encoding) "画像を準備中"
                                else "画像を添付（${body.images.size}/$limit 枚）"
                        },
                    Alignment.Center,
                ) {
                    if (ui.encoding) Spinner()
                    else
                        Icon(
                            Icons.Plus,
                            null,
                            Modifier.size(20.dp),
                            if (key != null && limit > 0) pi.textSecondary else pi.textTertiary,
                        )
                }
                // 作業中は、送ると割り込みになることを示す。モデルは待機中に選ぶ（/model なら作業中も開ける）。
                if (active)
                    Text(
                        "割り込みで送信",
                        Modifier.weight(1f, false).padding(end = 8.dp),
                        pi.textSecondary,
                        style = Type.body.copy(fontSize = 13.sp),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                else ModelLabel(groups, view, key != null, Modifier.weight(1f, false)) { show("model") }
                if (m.selected != null) UsageRings(m, groups, view, key != null) { show("usage") }
              }
                if (active && target != null) {
                    val stop = m.can(Operation.Abort(target))
                    Box(
                        Modifier.size(36.dp).clip(CircleShape).clickable(stop, role = Role.Button) {
                            m.abort(editor)
                        },
                        Alignment.Center,
                    ) {
                        Icon(
                            Icons.Stop,
                            "停止",
                            Modifier.size(22.dp),
                            pi.text.copy(alpha = if (stop) 1f else .4f),
                        )
                    }
                    Spacer(Modifier.width(2.dp))
                }
                Box(
                    Modifier.size(36.dp)
                        .clip(CircleShape)
                        .background(if (canSend) pi.accent else pi.surfaceStrong)
                        .clickable(canSend, role = Role.Button) {
                            if (builtin && name != null) run(name)
                            else editor?.let { m.send(it, body) }
                        },
                    Alignment.Center,
                ) {
                    Icon(
                        Icons.Send,
                        if (active && !builtin) "割り込む" else "送信",
                        Modifier.size(18.dp),
                        if (canSend) pi.onAccent else pi.textSecondary,
                    )
                }
            }
        }
    }
}

/** 添付・未確認の送信の画像（64dp）。タップで全画面、remove があれば右上で外す。 */
@Composable
private fun Thumbs(
    images: List<Pic>,
    modifier: Modifier = Modifier,
    remove: ((Int) -> Unit)? = null,
) {
    val pics = LocalPics.current
    Row(modifier.horizontalScroll(rememberScrollState()), Arrangement.spacedBy(8.dp)) {
        images.forEachIndexed { i, pic ->
            Box {
                PicTile(pic, "添付画像 ${i + 1}", compact = true, { pics.open(images, i) })
                if (remove != null)
                    Box(
                        Modifier.align(Alignment.TopEnd)
                            .padding(3.dp)
                            .size(22.dp)
                            .clip(CircleShape)
                            .background(Color.Black.copy(alpha = .55f))
                            .clickable(role = Role.Button) { remove(i) },
                        Alignment.Center,
                    ) {
                        Icon(Icons.Close, "添付画像 ${i + 1} を外す", Modifier.size(12.dp), Color.White)
                    }
            }
        }
    }
}

/** 入力欄の「Opus 5.5 · high ⌄」。タップでモデル・Thinking のシートを開く。 */
@Composable
private fun ModelLabel(
    groups: Map<String, List<Item>>,
    view: Map<String, String>,
    enabled: Boolean,
    modifier: Modifier = Modifier,
    open: () -> Unit,
) {
    val label =
        remember(view["model"], view["thinking"], groups["model"]) {
            val ref = view["model"]?.takeIf { it != "null" }?.let(::JSONObject)
            val info =
                groups["model"].orEmpty().map { JSONObject(it.json) }.find { same(it, ref) }
            val level = view["thinking"]?.let { JSONTokener(it).nextValue() as? String }.orEmpty()
            ref?.let {
                modelLabel(
                    it.str("id"),
                    info?.str("name"),
                    level,
                    info?.optJSONArray("thinkingLevels")?.length(),
                )
            }
        }
    if (label != null)
        Row(
            modifier
                .heightIn(min = 36.dp)
                .clip(RoundedCornerShape(8.dp))
                .clickable(enabled, "モデルと Thinking を変更", Role.Button, open)
                .padding(end = 6.dp)
                .clearAndSetSemantics { contentDescription = "モデル: $label" },
            Arrangement.spacedBy(3.dp),
            Alignment.CenterVertically,
        ) {
            Text(
                label,
                Modifier.weight(1f, false),
                pi.textSecondary,
                style = Type.body.copy(fontSize = 13.sp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Icon(Icons.Down, null, Modifier.size(12.dp), pi.textSecondary)
        }
}

/** 使用状況のリング 3 つ（コンテキスト・Claude・Codex）。タップで使用状況のシートを開く。 */
@Composable
private fun UsageRings(
    m: SessionModel,
    groups: Map<String, List<Item>>,
    view: Map<String, String>,
    enabled: Boolean,
    open: () -> Unit,
) {
    // 期限を過ぎた利用枠の数字は古い。1 分ごとに読み直して、不明に戻す。
    val now by
        produceState(System.currentTimeMillis()) {
            while (true) {
                delay(60_000)
                value = System.currentTimeMillis()
            }
        }
    val allowances = remember(groups["quota"], now) { m.allowances(now.toULong()) }
    val context =
        view["context"]
            ?.takeIf { it != "null" }
            ?.let(::JSONObject)
            ?.takeUnless { it.isNull("percent") }
            ?.optDouble("percent")
    val claude = allowances.find { it.source == "claude" }?.gauge
    val codex = allowances.find { it.source == "codex" }?.gauge
    fun say(name: String, percent: Double?) =
        "$name ${percent?.let { "${Math.round(it)}%" } ?: "不明"}"
    Row(
        Modifier.heightIn(min = 36.dp)
            .clip(RoundedCornerShape(8.dp))
            .clickable(enabled, "コンテキストと利用枠の詳細を開く", Role.Button, open)
            .padding(horizontal = 4.dp)
            .clearAndSetSemantics {
                contentDescription =
                    "使用状況: " +
                        listOf(say("コンテキスト", context), say("Claude", claude), say("Codex", codex))
                            .joinToString("、")
            },
        Arrangement.spacedBy(6.dp),
        Alignment.CenterVertically,
    ) {
        UsageRing(context) { Icon(Icons.Context, null, Modifier.size(12.dp), pi.textSecondary) }
        UsageRing(claude) { Icon(Icons.Claude, null, Modifier.size(12.dp), Color(0xFFD97757)) }
        UsageRing(codex) { Icon(Icons.OpenAI, null, Modifier.size(12.dp), pi.text) }
    }
}

/** `/` で始まる入力の候補。選んでも入力欄に入るだけで、実行は送信したとき。 */
@Composable
private fun CommandPalette(
    m: SessionModel,
    text: String,
    commands: List<Item>?,
    failed: Boolean,
    retry: () -> Unit,
    select: (String) -> Unit,
) {
    val names = remember(text, commands) { m.suggestions(text) }
    val known = remember(commands) { commands.orEmpty().associate { it.id to JSONObject(it.json) } }
    Column(
        Modifier.padding(bottom = 8.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(pi.surface)
    ) {
        Text(
            "コマンド",
            Modifier.padding(start = 14.dp, top = 10.dp, end = 14.dp, bottom = 4.dp).semantics {
                heading()
            },
            pi.textSecondary,
            style = Type.caption,
        )
        if (failed)
            Row(Modifier.padding(start = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "コマンド一覧を取得できませんでした",
                    Modifier.weight(1f).semantics { liveRegion = LiveRegionMode.Polite },
                    pi.textSecondary,
                    style = Type.caption,
                )
                TextAction("再試行", retry)
            }
        Column(Modifier.heightIn(max = 220.dp).verticalScroll(rememberScrollState())) {
            names.forEach { name ->
                val source = known[name]?.str("source").orEmpty()
                val description =
                    if (source == "builtin") builtinCommands[name].orEmpty()
                    else known[name]?.str("description").orEmpty()
                Row(
                    Modifier.fillMaxWidth()
                        .heightIn(min = 56.dp)
                        .clickable(onClickLabel = "入力欄に入れる", role = Role.Button) { select(name) }
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                    Arrangement.spacedBy(8.dp),
                    Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("/$name", style = Type.mono.copy(fontSize = 14.sp), maxLines = 1)
                        Text(
                            description.ifEmpty { "説明なし" },
                            color = pi.textSecondary,
                            style = Type.caption,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    Text(commandKind(source), color = pi.textSecondary, style = Type.meta)
                }
            }
            if (names.isEmpty() && !failed)
                Text(
                    "一致するコマンドはありません",
                    Modifier.padding(14.dp),
                    pi.textSecondary,
                    style = Type.caption,
                )
        }
    }
}
