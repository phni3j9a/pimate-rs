package dev.pimate.app

import android.content.Intent
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.*
import uniffi.pimate_ffi.*

@Composable
internal fun Page(
    title: String,
    close: () -> Unit,
    footer: (@Composable ColumnScope.() -> Unit)? = null,
    // 行の間。設定・マシン詳細は行が自分の高さを持つので 0。
    gap: Dp = 4.dp,
    centered: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    FullScreen(close) {
        Column(Modifier.fillMaxSize().background(pi.background).safeDrawingPadding().imePadding()) {
            if (centered)
                Box(Modifier.fillMaxWidth().heightIn(min = 52.dp), Alignment.Center) {
                    IconAction(
                        Icons.Close,
                        "閉じる",
                        close,
                        Modifier.align(Alignment.CenterStart),
                        tint = pi.text,
                    )
                    Text(title, Modifier.semantics { heading() }, style = Type.sheet)
                }
            else if (title.isNotEmpty())
                Row(
                    Modifier.heightIn(min = 52.dp).padding(horizontal = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconAction(Icons.Back, "戻る", close, tint = pi.text)
                    Text(
                        title,
                        Modifier.weight(1f).offset(x = (-2).dp).semantics { heading() },
                        style = Type.sheet,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            Column(
                Modifier.fillMaxWidth()
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = gutter),
                verticalArrangement = Arrangement.spacedBy(gap),
            ) {
                content()
                Spacer(Modifier.height(24.dp))
            }
            if (footer != null)
                Column(
                    Modifier.fillMaxWidth().padding(horizontal = gutter, vertical = 8.dp),
                    content = footer,
                )
        }
    }
}

@Composable
internal fun Hint(text: String) {
    Text(text, color = pi.textSecondary, style = Type.caption)
}

/** Approval belongs to the action that opened the dialog, not the next occupant of this row. */
@Composable
internal fun ConfirmAction(
    identity: Any,
    label: String,
    detail: String,
    button: String = label,
    enabled: Boolean = true,
    title: String = label,
    modifier: Modifier = Modifier,
    color: Color = pi.failedText,
    weight: FontWeight = FontWeight.SemiBold,
    style: TextStyle = Type.body,
    onConfirm: () -> Unit,
) {
    var pending by
        remember(identity, label, detail, enabled) { mutableStateOf<(() -> Unit)?>(null) }
    TextAction(button, { pending = onConfirm }, modifier, enabled, color, weight, style)
    pending?.let { approved ->
        AlertDialog(
            { pending = null },
            title = { Text(title, style = Type.sheet) },
            text = { Text(detail) },
            confirmButton = {
                TextAction(
                    label,
                    {
                        pending = null
                        approved()
                    },
                    color = color,
                )
            },
            dismissButton = { TextAction("キャンセル", { pending = null }) },
            containerColor = pi.background,
        )
    }
}

internal suspend fun checked(error: (String) -> Unit, task: suspend () -> Unit) {
    try {
        withContext(Dispatchers.Main.immediate) { task() }
    } catch (e: CoreException) {
        error(failure(e))
    } catch (e: JSONException) {
        error("PC の応答を読み取れませんでした")
    }
}

/** リンクを外のアプリで開く。開けるアプリがなければ false。 */
internal fun openLink(context: android.content.Context, url: String): Boolean =
    try {
        context.startActivity(Intent(Intent.ACTION_VIEW, android.net.Uri.parse(url)))
        true
    } catch (_: android.content.ActivityNotFoundException) {
        false
    }

@Composable
internal fun External(label: String, url: String) {
    val context = LocalContext.current
    var unavailable by remember { mutableStateOf(false) }
    TextAction(label, { unavailable = !openLink(context, url) })
    if (unavailable) SheetError("リンクを開けるアプリがありません")
}

@Composable
internal fun Toggle(label: String, value: Boolean, detail: String? = null, set: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(label, style = Type.lead)
            if (detail != null) Hint(detail)
        }
        Switch(
            value,
            set,
            Modifier.semantics { contentDescription = label },
            // 大きさのある中身を渡すと、オフでもつまみが 24dp になる（モックの形）。
            thumbContent = { Spacer(Modifier.size(SwitchDefaults.IconSize)) },
            colors =
                SwitchDefaults.colors(
                    checkedThumbColor = pi.onAccent,
                    checkedTrackColor = pi.accent,
                    uncheckedThumbColor = pi.onAccent,
                    uncheckedTrackColor = pi.control,
                    uncheckedBorderColor = Color.Transparent,
                ),
        )
    }
}

/** 同じアカウントでつなぐ2台。図だけで接続済みとは伝えない。 */
@Composable
private fun DeviceLink(connected: Boolean, first: Boolean = false) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = if (first) 0.dp else 18.dp),
        horizontalArrangement = if (first) Arrangement.Start else Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(48.dp)
                .background(
                    pi.surface,
                    androidx.compose.foundation.shape.RoundedCornerShape(14.dp),
                ),
            Alignment.Center,
        ) {
            Icon(Icons.Phone, null, Modifier.size(24.dp), pi.textCode)
        }
        Row(
            Modifier.padding(horizontal = 6.dp),
            Arrangement.spacedBy(3.dp),
            Alignment.CenterVertically,
        ) {
            repeat(4) { Dot(pi.textTertiary, 3.dp) }
            if (connected) Dot(pi.done, 8.dp)
            repeat(4) { Dot(pi.textTertiary, 3.dp) }
        }
        Box(
            Modifier.size(48.dp)
                .background(
                    pi.surface,
                    androidx.compose.foundation.shape.RoundedCornerShape(14.dp),
                ),
            Alignment.Center,
        ) {
            Icon(Icons.Laptop, null, Modifier.size(24.dp), pi.textCode)
        }
    }
}

@Composable
internal fun SetupGuide(step: Int, close: () -> Unit, next: () -> Unit, scan: () -> Unit) {
    val context = LocalContext.current
    Page(
        if (step == 1) "Tailscale でつなぐ" else "PC に PiMate を入れる",
        close,
        footer = {
            if (step == 1) PrimaryButton("次へ：PC に PiMate を入れる", next, Modifier.fillMaxWidth())
            else PrimaryButton("QRコードを読み取る", scan, Modifier.fillMaxWidth())
        },
    ) {
        Hint("ペアリングの準備　$step / 3")
        if (step == 1) {
            Text("スマホと PC を同じ Tailscale のネットワークにつなぎます。", style = Type.lead)
            DeviceLink(false)
            Label("このスマホ")
            Text("Tailscale のアプリを入れてログインし、VPN 接続を許可してください。")
            Label("PC")
            Text("PC にも Tailscale を入れ、スマホと同じアカウントでログインします。")
            External("PC へのインストール方法", "https://tailscale.com/download")
            Hint("スマホの Tailscale の一覧に PC が出ていれば準備完了です。")
            External(
                "Play ストアで Tailscale を開く",
                "https://play.google.com/store/apps/details?id=com.tailscale.ipn",
            )
        } else {
            val install = "pi install npm:@phni3j9a/pimate"
            Label("1　ターミナルで実行します")
            CommandPill(install)
            CommandPill("pi")
            Label("2　Pi の入力欄で実行します")
            PiTerminal(Modifier.padding(vertical = 6.dp))
            CommandPill("/pimate setup")
            Hint("QR コードが表示されます。ペアリングし直すときは /pimate pair で出し直せます。")
            TextAction(
                "この手順を PC に送る",
                {
                    context.startActivity(
                        Intent.createChooser(
                            Intent(Intent.ACTION_SEND)
                                .setType("text/plain")
                                .putExtra(
                                    Intent.EXTRA_TEXT,
                                    "ターミナルで：\n$install\npi\nPi の入力欄で：\n/pimate setup",
                                ),
                            null,
                        )
                    )
                },
            )
        }
    }
}

@Composable
internal fun FirstPage(
    m: SessionModel,
    host: Host,
    close: () -> Unit,
    spawn: () -> Unit,
    select: (String) -> Unit,
) {
    val rows = m.rowsFor(host.id)
    Page(
        "",
        close,
        footer = {
            if (rows.firstStep() == FirstStep.LIST) Hint("一覧は、チャットで右へスワイプするといつでも開けます")
            if (rows.firstStep() == FirstStep.EMPTY)
                PrimaryButton("新しい Pi を起動", spawn, Modifier.fillMaxWidth())
            if (rows.firstStep() != FirstStep.LIST)
                TextAction("一覧を開く", close, Modifier.fillMaxWidth())
        },
    ) {
        Spacer(Modifier.height(32.dp))
        DeviceLink(rows.firstStep() in listOf(FirstStep.EMPTY, FirstStep.LIST), first = true)
        Spacer(Modifier.height(12.dp))
        when (rows.firstStep()) {
            FirstStep.LOADING -> Text("PC の Pi を確認しています…")
            FirstStep.OFFLINE -> Text("PC に接続していません")
            else -> {
                Text("${host.name} とつながりました", style = Type.title)
                if (rows.firstStep() == FirstStep.EMPTY) {
                    Text("まだ起動中の Pi はありません。このスマホから PC のフォルダを選んで、新しい Pi を起動できます。")
                    CommandPill("pi")
                    Hint("PC のターミナルで起動した Pi も、ここに表示されます")
                } else {
                    Text("選ぶとチャットを開きます。", color = pi.textSecondary, style = Type.body)
                    Label("起動中の Pi")
                    rows.groups["instance"].orEmpty().forEach {
                        val o = JSONObject(it.json)
                        InstanceRow(
                            o,
                            false,
                            when {
                                o.has("failureText") -> "失敗: ${o.str("failureText")}"
                                o.str("status") == "waiting_input" ->
                                    "入力待ち: Pi が回答を待っています"
                                o.str("section") == "waiting" -> "サブエージェントの確認が必要です"
                                else -> null
                            },
                            ago(o.optLong("updatedAt"), System.currentTimeMillis()),
                            inset = 0.dp,
                        ) {
                            select(it.id)
                        }
                    }
                    TextAction("新しい Pi を起動", spawn)
                    TextAction("一覧を開く", close)
                }
            }
        }
    }
}
