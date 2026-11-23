package dev.pimate.app

import android.content.Intent
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.*
import uniffi.pimate_ffi.*

@Composable
internal fun Page(
    title: String,
    close: () -> Unit,
    footer: (@Composable ColumnScope.() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    FullScreen(close) {
        Column(Modifier.fillMaxSize().background(pi.background).safeDrawingPadding().imePadding()) {
            Row(Modifier.heightIn(min = 52.dp), verticalAlignment = Alignment.CenterVertically) {
                IconAction(Icons.Back, "戻る", close, tint = pi.text)
                Text(title, Modifier.semantics { heading() }, style = Type.header)
            }
            Column(
                Modifier.fillMaxWidth()
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = gutter),
                verticalArrangement = Arrangement.spacedBy(4.dp),
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
    onConfirm: () -> Unit,
) {
    var pending by
        remember(identity, label, detail, enabled) { mutableStateOf<(() -> Unit)?>(null) }
    TextAction(button, { pending = onConfirm }, modifier, enabled = enabled, color = pi.failedText)
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
                    color = pi.failedText,
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

@Composable
internal fun External(label: String, url: String) {
    val context = LocalContext.current
    var unavailable by remember { mutableStateOf(false) }
    TextAction(
        label,
        {
            try {
                context.startActivity(Intent(Intent.ACTION_VIEW, android.net.Uri.parse(url)))
            } catch (_: android.content.ActivityNotFoundException) {
                unavailable = true
            }
        },
    )
    if (unavailable) SheetError("リンクを開けるアプリがありません")
}

@Composable
internal fun Toggle(label: String, value: Boolean, set: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, Modifier.weight(1f))
        Switch(value, set, Modifier.semantics { contentDescription = label })
    }
}

/** 同じアカウントでつなぐ2台。図だけで接続済みとは伝えない。 */
@Composable
private fun DeviceLink(connected: Boolean) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 18.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(50.dp)
                .background(
                    pi.surface,
                    androidx.compose.foundation.shape.RoundedCornerShape(14.dp),
                ),
            Alignment.Center,
        ) {
            Icon(Icons.Phone, null, Modifier.size(24.dp), pi.textCode)
        }
        Row(
            Modifier.padding(horizontal = 12.dp),
            Arrangement.spacedBy(6.dp),
            Alignment.CenterVertically,
        ) {
            repeat(3) { Dot(pi.textTertiary, 3.dp) }
            if (connected) Icon(Icons.Check, null, Modifier.size(18.dp), pi.accentText)
            repeat(3) { Dot(pi.textTertiary, 3.dp) }
        }
        Box(
            Modifier.size(50.dp)
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
        host.name,
        close,
        footer = {
            if (rows.firstStep() in listOf(FirstStep.EMPTY, FirstStep.LIST))
                PrimaryButton("新しい Pi を起動", spawn, Modifier.fillMaxWidth())
            TextAction("一覧を開く", close, Modifier.fillMaxWidth())
        },
    ) {
        when (rows.firstStep()) {
            FirstStep.LOADING -> Text("PC の Pi を確認しています…")
            FirstStep.OFFLINE -> Text("PC に接続していません")
            else -> {
                Text("${host.name} とつながりました", style = Type.title)
                if (rows.firstStep() == FirstStep.EMPTY) {
                    DeviceLink(true)
                    Text("まだ起動中の Pi はありません。このスマホから PC のフォルダを選んで、新しい Pi を起動できます。")
                    CommandPill("pi")
                    Hint("PC のターミナルで起動した Pi も、ここに表示されます")
                } else {
                    Hint("選ぶとチャットを開きます。")
                    Label("起動中の Pi")
                    rows.groups["instance"].orEmpty().forEach {
                        val o = JSONObject(it.json)
                        InstanceRow(
                            o,
                            false,
                            if (o.str("status") == "waiting_input") "入力待ち" else null,
                            ago(o.optLong("updatedAt"), System.currentTimeMillis()),
                        ) {
                            select(it.id)
                        }
                    }
                    Hint("一覧は、チャットで右へスワイプするといつでも開けます")
                }
            }
        }
    }
}
