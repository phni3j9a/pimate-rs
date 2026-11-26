package dev.pimate.app

import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.*
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.*
import uniffi.pimate_ffi.*

private fun Rows.hostValue(name: String) = groups["host"].orEmpty().find { it.id == name }?.json

private fun Rows.info() = hostValue("info")?.let(::JSONObject) ?: JSONObject()

private fun Rows.summary(): String {
    val connection = hostValue("connection")
    if (connection != "\"connected\"")
        return stopped(connection)?.label ?: if (connection == "\"offline\"") "オフライン" else "接続しています"
    return listOfNotNull(
            "接続中",
            when (val p = info().str("platform")) {
                "" -> null
                "darwin" -> "macOS"
                "linux" -> "Linux"
                "win32" -> "Windows"
                else -> p
            },
            if (hostValue("listKnown") == "true") "Pi ${groups["instance"].orEmpty().size} 件"
            else null,
        )
        .joinToString(" · ")
}

internal fun JSONArray?.strings() =
    List(this?.length() ?: 0) { this?.opt(it) }.filterIsInstance<String>()

@Composable
internal fun NewPage(
    m: SessionModel,
    initial: Host?,
    close: () -> Unit,
    select: (Host, String) -> Unit,
) {
    var hostId by rememberSaveable { mutableStateOf(initial?.id.orEmpty()) }
    val h = m.hosts.find { it.id == hostId }
    key(h?.id) {
        val form = remember { SpawnForm(hostId, m.settings.askTool) }
        var folders by remember { mutableStateOf<JSONObject?>(null) }
        var models by remember { mutableStateOf<JSONObject?>(null) }
        var dirs by remember(form.cwd) { mutableStateOf<List<String>?>(null) }
        var modelPicker by remember { mutableStateOf(false) }
        var error by remember { mutableStateOf("") }
        var busy by remember { mutableStateOf(false) }
        var outcome by remember { mutableStateOf("") }
        val scope = rememberCoroutineScope()
        LaunchedEffect(hostId) {
            checked({ error = it }) {
                folders = JSONObject(m.machineRequest(hostId, Operation.Folders))
                if (form.cwd.isEmpty()) form.folder(folders!!.str("home"))
            }
            checked({ error = it }) { models = m.hostModels(hostId) }
        }
        val recent = folders?.optJSONArray("folders").objects()
        fun folder(path: String) =
            form.folder(path, recent.any { it.str("path") == path && it.optBoolean("trusted") })
        Page(
            "新しい Pi",
            close,
            footer = {
                PrimaryButton(
                    if (busy) "起動を確認しています…" else "起動",
                    {
                        if (!busy && h != null) {
                            busy = true
                            error = ""
                            scope.launch {
                                try {
                                    checked({ error = it }) {
                                        when (val r = m.spawn(form)) {
                                            is SpawnOutcome.Ready -> select(h, r.instance)
                                            is SpawnOutcome.NotListed ->
                                                outcome = "起動を受け付けましたが、一覧では確認できません。一覧で確認してください。"
                                            is SpawnOutcome.MessageUnconfirmed ->
                                                outcome =
                                                    "Pi は起動しましたが、最初のメッセージは未確認です。" +
                                                        r.attach.orEmpty()
                                        }
                                    }
                                } finally {
                                    busy = false
                                }
                            }
                        }
                    },
                    Modifier.fillMaxWidth(),
                    enabled = h != null && form.cwd.isNotBlank() && !busy && outcome.isEmpty(),
                )
            },
        ) {
            Label("マシン", top = 8.dp)
            m.hosts.forEach {
                val rows = m.rowsFor(it.id)
                val connection = rows.hostValue("connection")
                val failed = it.needsPairing || stopped(connection) != null
                ListRow(
                    it.name,
                    // つながっていないマシンは、理由を補足に出す。
                    when {
                        it.needsPairing -> "再ペアリングが必要"
                        connection != "\"connected\"" -> rows.summary()
                        else -> ""
                    },
                    if (failed) pi.failedText else pi.textSecondary,
                    enabled = !busy && !it.needsPairing,
                    onClick = { hostId = it.id },
                    leading = { HostMark(connection, failed) },
                    trailing =
                        if (h != it) null
                        else {
                            { Icon(Icons.Check, "選択中", Modifier.size(18.dp), pi.accentText) }
                        },
                )
            }
            Label("フォルダ")
            OutlinedTextField(
                form.cwd,
                { folder(it) },
                Modifier.fillMaxWidth(),
                label = { Text("フォルダのパス") },
                singleLine = true,
                shape = CircleShape,
                enabled = !busy,
            )
            if (folders == null && error.isEmpty()) Hint("フォルダを読み込み中…")
            recent.take(8).forEach {
                val path = it.str("path")
                MenuRow(tilde(path), if (form.cwd == path) "選択中" else "", !busy) { folder(path) }
            }
            if (m.rowsFor(hostId).info().optBoolean("dirs")) {
                TextAction(
                    "子フォルダを表示",
                    {
                        val path = form.cwd
                        scope.launch {
                            checked({ error = it }) {
                                val r = JSONObject(m.machineRequest(hostId, Operation.Dirs(path)))
                                if (form.cwd == path) dirs = r.optJSONArray("dirs").strings()
                            }
                        }
                    },
                    enabled = !busy && form.cwd.isNotBlank(),
                )
                dirs?.let { names ->
                    if (names.isEmpty()) Hint("子フォルダはありません")
                    names.forEach {
                        MenuRow(it, "", !busy, next = true) {
                            folder(form.cwd.trimEnd('/') + "/" + it)
                        }
                    }
                }
            }
            Toggle("このフォルダを信頼する", form.trust) { if (!busy) form.trust = it }
            Hint("信頼すると、このフォルダの設定と Extension を読み込みます。")
            Label("モデル")
            MenuRow(
                form.model?.str("id") ?: "PC の既定",
                form.thinking
                    ?: (if (form.model == null)
                            models?.optJSONObject("default")?.optJSONObject("model")?.str("id")
                        else null)
                        .orEmpty(),
                !busy && models != null,
                next = true,
            ) {
                modelPicker = !modelPicker
            }
            if (modelPicker) {
                val favorites = models?.optJSONArray("scoped").objects()
                TextAction(
                    "PC の既定",
                    {
                        form.model = null
                        form.thinking = null
                        modelPicker = false
                    },
                )
                models
                    ?.optJSONArray("models")
                    .objects()
                    .sortedByDescending { model -> favorites.any { same(it, model) } }
                    .forEach { model ->
                        MenuRow(
                            model.str("name").ifEmpty { model.str("id") },
                            (if (favorites.any { same(it, model) }) "★ " else "") +
                                model.str("provider"),
                            !busy,
                        ) {
                            form.model =
                                JSONObject()
                                    .put("provider", model.str("provider"))
                                    .put("id", model.str("id"))
                            form.thinking = null
                            modelPicker = false
                        }
                    }
            }
            models
                ?.optJSONArray("models")
                .objects()
                .find { same(it, form.model) }
                ?.optJSONArray("thinkingLevels")
                .strings()
                .forEach { level ->
                    MenuRow(level, if (form.thinking == level) "選択中" else "", !busy) {
                        form.thinking = level
                    }
                }
            Toggle("質問ツール（pimate_ask）", form.askTool) { if (!busy) form.askTool = it }
            OutlinedTextField(
                form.message,
                { form.message = it },
                Modifier.fillMaxWidth(),
                label = { Text("最初のメッセージ（任意）") },
                enabled = !busy,
            )
            SheetError(error)
            if (outcome.isNotEmpty()) Hint(outcome)
        }
    }
}

/** マシンの状態の印（12dp の列）。色だけに頼らず、隣の文字でも状態を示す。 */
@Composable
private fun HostMark(connection: String?, failed: Boolean) {
    Box(Modifier.width(12.dp), Alignment.Center) {
        when {
            failed -> Dot(pi.failed, 8.dp)
            connection == "\"connected\"" -> Dot(pi.done, 8.dp)
            connection == "\"offline\"" -> Dot(pi.textSecondary, 8.dp, Modifier.alpha(.5f))
            else -> Spinner(12.dp)
        }
    }
}

/** 設定・マシン詳細の行。1行は 52dp、補足つきは 60dp（TS 版 0.1.23 の settings.tsx / machine.tsx と同じ寸法）。 */
@Composable
private fun ListRow(
    title: String,
    sub: String = "",
    subColor: Color = pi.textSecondary,
    enabled: Boolean = true,
    minHeight: Dp = if (sub.isEmpty()) 52.dp else 60.dp,
    onClick: (() -> Unit)? = null,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = minHeight)
            .then(
                if (onClick == null) Modifier
                else
                    Modifier.clip(RoundedCornerShape(12.dp))
                        .clickable(enabled, role = Role.Button, onClick = onClick)
            )
            // 右の操作はタップ領域が 48dp あるので、その行も 60dp に収まる余白にする。
            .padding(vertical = 6.dp),
        Arrangement.spacedBy(12.dp),
        Alignment.CenterVertically,
    ) {
        leading?.invoke()
        // 押せない行は、名前と補足をひとまとまりで読み上げる（右の操作は別）。押せる行は行全体がまとまる。
        Column(
            Modifier.weight(1f)
                .then(
                    if (onClick == null) Modifier.semantics(mergeDescendants = true) {}
                    else Modifier
                )
        ) {
            Text(title, color = if (enabled) pi.text else pi.textSecondary, style = Type.lead)
            if (sub.isNotEmpty()) Text(sub, color = subColor, style = Type.caption)
        }
        trailing?.invoke()
    }
}

/** 外のアプリで開く行。右の印で、アプリの外へ出ることを示す。 */
@Composable
private fun LinkRow(label: String, url: String, leading: (@Composable () -> Unit)? = null) {
    val context = LocalContext.current
    var unavailable by remember { mutableStateOf(false) }
    ListRow(
        label,
        onClick = { unavailable = !openLink(context, url) },
        leading = leading,
        trailing = { Icon(Icons.External, null, Modifier.size(14.dp), pi.textSecondary) },
    )
    if (unavailable) SheetError("リンクを開けるアプリがありません")
}

/** 見出しと、右に寄せた3択（テーマ・文字サイズ）。 */
@Composable
private fun SegmentRow(
    label: String,
    options: List<Pair<String, String>>,
    selected: String,
    select: (String) -> Unit,
) {
    val group = Modifier.semantics { contentDescription = label }
    // 文字が大きいと 192dp の中で選択肢が切れるので、見出しの下に幅いっぱいで置く。
    if (LocalDensity.current.fontScale > 1.15f)
        Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), Arrangement.spacedBy(6.dp)) {
            Text(label, style = Type.lead)
            Segment(options, selected, group.fillMaxWidth(), compact = true, select = select)
        }
    else
        Row(
            Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(vertical = 8.dp),
            Arrangement.spacedBy(12.dp),
            Alignment.CenterVertically,
        ) {
            Text(label, Modifier.weight(1f), style = Type.lead)
            Segment(options, selected, group.width(192.dp), compact = true, select = select)
        }
}

// 行の左端・右端にそろえる文字の操作（TextAction の左右の余白 14dp を打ち消す）。
private val flush = Modifier.offset(x = (-14).dp)
private val flushEnd = Modifier.offset(x = 14.dp)

@Composable
internal fun SettingsPage(
    m: SessionModel,
    close: () -> Unit,
    machine: (String) -> Unit,
    pair: () -> Unit,
) {
    val s = m.settings
    val context = LocalContext.current
    val version = remember {
        context.packageManager.getPackageInfo(context.packageName, 0).versionName.orEmpty()
    }
    var update by remember { mutableStateOf<AppUpdate?>(null) }
    LaunchedEffect(version) { update = checkAppUpdate(version) }
    Page("設定", close, gap = 0.dp) {
        Label("マシン", top = 8.dp)
        m.hosts.forEach { h ->
            val rows = m.rowsFor(h.id)
            val connection = rows.hostValue("connection")
            val failed = h.needsPairing || stopped(connection) != null
            val state = if (h.needsPairing) "再ペアリングが必要" else rows.summary()
            ListRow(
                h.name,
                if (connection == "\"connected\"") state else "$state · ${h.address}",
                if (failed) pi.failedText else pi.textSecondary,
                onClick = { machine(h.id) },
                leading = { HostMark(connection, failed) },
                trailing = { Icon(Icons.Right, null, Modifier.size(16.dp), pi.textSecondary) },
            )
        }
        Row(
            Modifier.fillMaxWidth()
                .heightIn(min = 52.dp)
                .clip(RoundedCornerShape(12.dp))
                .clickable(role = Role.Button, onClick = pair),
            Arrangement.spacedBy(12.dp),
            Alignment.CenterVertically,
        ) {
            Icon(Icons.Plus, null, Modifier.size(16.dp), pi.accentText)
            Text(
                if (m.hosts.any { it.needsPairing }) "ペアリングし直す・マシンを追加"
                else "マシンを追加（ペアリング）",
                color = pi.accentText,
                style = Type.lead,
            )
        }
        Label("新しい Pi の既定", bottom = 0.dp)
        Toggle("質問ツール（pimate_ask）", s.askTool, s::chooseAskTool)
        Label("表示", bottom = 0.dp)
        SegmentRow(
            "テーマ",
            listOf("SYSTEM" to "システム", "LIGHT" to "ライト", "DARK" to "ダーク"),
            s.theme.name,
        ) {
            s.chooseTheme(ThemeMode.valueOf(it))
        }
        SegmentRow(
            "文字サイズ",
            fontScales.map { it.second.toString() to it.first },
            s.scale.toString(),
        ) {
            s.chooseScale(it.toFloat())
        }
        Label("情報", bottom = 0.dp)
        val available = update as? AppUpdate.Available
        var unavailable by remember { mutableStateOf(false) }
        ListRow(
            "PiMate $version",
            when (update) {
                null -> "更新を確認中…"
                AppUpdate.Latest -> "最新です"
                AppUpdate.Unknown -> "更新を確認できませんでした"
                is AppUpdate.Available -> "${available?.version} に更新できます"
            },
            if (available != null) pi.accentText else pi.textSecondary,
            onClick = available?.let { { unavailable = !openLink(context, it.url) } },
            // マークは 1024 の版面の中の 920×816（icon-kit）。20dp の枠に同じ比率で置く。
            leading = {
                Image(
                    painterResource(R.drawable.mark),
                    null,
                    Modifier.size(20.dp).padding(horizontal = 1.dp, vertical = 2.dp),
                )
            },
            trailing =
                available?.let {
                    { Icon(Icons.External, null, Modifier.size(14.dp), pi.accentText) }
                },
        )
        if (unavailable) SheetError("リンクを開けるアプリがありません")
        LinkRow("GitHub", REPO) { Icon(Icons.GitHub, null, Modifier.size(20.dp), pi.text) }
        LinkRow("不具合を報告", REPORT_ISSUE)
    }
}

private fun day(ms: Long) = android.text.format.DateFormat.format("yyyy/M/d", ms).toString()

@Composable
internal fun MachinePage(m: SessionModel, host: Host, close: () -> Unit, pair: () -> Unit) {
    val id = host.id
    val rows = m.rowsFor(id)
    val caps = rows.info()
    val connection = rows.hostValue("connection")
    val online = connection == "\"connected\""
    var login by rememberSaveable { mutableStateOf(false) }
    if (login) {
        ProviderPage(m, id) { login = false }
        return
    }
    var check by remember(online) { mutableStateOf<JSONObject?>(null) }
    var gh by remember(online) { mutableStateOf<JSONObject?>(null) }
    var devices by remember(online) { mutableStateOf<List<JSONObject>?>(null) }
    var retry by remember { mutableIntStateOf(0) }
    var error by remember { mutableStateOf("") }
    var note by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()
    suspend fun request(op: Operation) = JSONObject(m.machineRequest(id, op))
    LaunchedEffect(id, online, retry, caps.toString()) {
        error = ""
        if (online)
            coroutineScope {
                if (caps.optBoolean("pairedDevices"))
                    launch {
                        checked({ error = it }) {
                            devices = request(Operation.Devices).getJSONArray("devices").objects()
                        }
                    }
                if (caps.optBoolean("githubStatus"))
                    launch {
                        checked({ error = it }) {
                            gh = request(Operation.GithubStatus).getJSONObject("status")
                        }
                    }
                if (caps.optBoolean("packageUpdate"))
                    launch { checked({ error = it }) { check = request(Operation.UpdateCheck) } }
            }
    }
    fun operate(action: suspend () -> Unit) {
        if (busy || m.hosts.none { it.id == host.id && it.deviceId == host.deviceId }) return
        busy = true
        note = ""
        error = ""
        scope.launch {
            try {
                checked({ error = it }, action)
            } finally {
                busy = false
            }
        }
    }
    Page(host.name, close, gap = 0.dp) {
        Label("状態", top = 8.dp, bottom = 0.dp)
        val failed = host.needsPairing || stopped(connection) != null
        Row(
            Modifier.fillMaxWidth().heightIn(min = 52.dp),
            Arrangement.spacedBy(12.dp),
            Alignment.CenterVertically,
        ) {
            HostMark(connection, failed)
            Text(
                if (host.needsPairing) "再ペアリングが必要" else rows.summary(),
                Modifier.weight(1f),
                if (failed) pi.failedText else pi.text,
                style = Type.lead,
            )
            if (host.needsPairing || stopped(connection)?.repair == true)
                TextAction("ペアリングし直す", pair, flushEnd, weight = FontWeight.Normal, style = Type.lead)
        }
        ListRow(
            host.address,
            devices
                ?.find { it.optBoolean("current") }
                ?.optLong("createdAt")
                ?.takeIf { it > 0 }
                ?.let { "${day(it)} にペアリング" }
                .orEmpty(),
        )
        Label("PC の PiMate", bottom = 0.dp)
        val current = check?.str("current").orEmpty().ifEmpty { caps.str("version") }
        val latest = check?.str("latest").orEmpty()
        val newer = latest != "" && latest != current
        val canUpdate =
            online && newer && caps.optBoolean("packageUpdate") &&
                check?.optBoolean("canUpdate") == true
        // Host が自分では更新できない（常駐サービスでない・公開パッケージでない）。PC で操作してもらう。
        val manual = check?.str("reason").orEmpty().takeIf { online && (newer || latest == "") }
        ListRow(
            if (current.isEmpty()) "PiMate" else "現在 $current",
            when {
                !online -> "接続していません"
                !caps.optBoolean("packageUpdate") -> "PC の PiMate を更新すると使えます"
                check == null -> if (error.isEmpty()) "確認中…" else "確認できませんでした"
                latest == "" -> "最新版を確認できませんでした"
                newer -> "$latest に更新できます"
                else -> "最新です"
            },
            trailing =
                if (!canUpdate) null
                else {
                    {
                        ConfirmAction(
                            host to latest,
                            "更新",
                            "${host.name} を実行時の最新版に更新します（確認時点の最新版: $latest）。接続が一時的に切れる場合があります。",
                            enabled = !busy,
                            title = "PC の PiMate を更新しますか？",
                            modifier = flushEnd,
                            color = pi.accentText,
                            style = Type.lead,
                        ) {
                            operate {
                                val r = request(Operation.Update)
                                note = "${r.getString("to")} に更新しました"
                                retry++
                            }
                        }
                    }
                },
        )
        if (manual == "not_service") {
            Hint("PC で実行してください")
            CommandPill("pi update npm:@phni3j9a/pimate", Modifier.padding(vertical = 8.dp))
        } else if (!manual.isNullOrEmpty()) {
            Hint("開発版のため PC で更新してください")
            Spacer(Modifier.height(8.dp))
        }
        if (online && caps.optBoolean("hostRestart"))
            ConfirmAction(
                host,
                "Host を再起動",
                "${host.name} との接続が一時的に切れる場合があります。",
                enabled = !busy,
                modifier = flush,
                color = pi.accentText,
                weight = FontWeight.Normal,
                style = Type.lead,
            ) {
                operate {
                    request(Operation.Restart)
                    note = "再起動を受け付けました"
                }
            }
        Label("プロバイダー", bottom = 0.dp)
        val canLogin = online && caps.optBoolean("authSession")
        ListRow(
            "プロバイダーのログイン",
            when {
                !online -> "接続していません"
                !canLogin -> "PC の PiMate を更新すると使えます"
                else -> ""
            },
            enabled = canLogin && !busy,
            minHeight = 60.dp,
            onClick = { login = true },
            trailing =
                if (!canLogin) null
                else {
                    { Icon(Icons.Right, null, Modifier.size(16.dp), pi.textSecondary) }
                },
        )
        // 以下の2欄は、接続中で PC が対応しているときだけ出す。
        if (online && caps.optBoolean("githubStatus")) {
            Label("GitHub", bottom = 0.dp)
            val status = gh
            ListRow(githubStatusText(status))
            if (status != null && status.optBoolean("installed") && !status.optBoolean("loggedIn"))
                CommandPill("gh auth login", Modifier.padding(bottom = 8.dp))
        }
        if (online && caps.optBoolean("pairedDevices")) {
            Label("ペアリング済みの端末", bottom = 0.dp)
            if (devices == null) Hint(if (error.isEmpty()) "確認中…" else "端末一覧は未確認です")
            devices.orEmpty().forEach { d ->
                val seen = d.optLong("lastSeenAt").takeIf { it > 0 }?.let { "最終接続 ${day(it)}" }
                val self = d.optBoolean("current")
                ListRow(
                    d.str("name"),
                    listOfNotNull(if (self) "このスマホ" else null, seen).joinToString(" · "),
                    trailing =
                        if (self) null
                        else {
                            {
                                val deviceId = d.getString("deviceId")
                                ConfirmAction(
                                    host to deviceId,
                                    "端末の取り消し",
                                    "${d.str("name")} はこの PC に接続できなくなります。",
                                    button = "取り消し",
                                    enabled = !busy,
                                    modifier = flushEnd,
                                    weight = FontWeight.Normal,
                                    style = Type.lead,
                                ) {
                                    operate {
                                        request(Operation.Revoke(deviceId))
                                        retry++
                                        note = "登録を取り消しました"
                                    }
                                }
                            }
                        },
                )
            }
        }
        SheetError(error)
        if (note.isNotEmpty()) Hint(note)
        Spacer(Modifier.height(28.dp))
        if (error.isNotEmpty())
            TextAction(
                "状態を再読み込み",
                { retry++ },
                flush,
                enabled = online && !busy,
                weight = FontWeight.Normal,
                style = Type.lead,
            )
        ConfirmAction(
            host,
            "このマシンを削除",
            "このスマホに保存した ${host.name} の登録と下書きを削除します。",
            enabled = !busy,
            modifier = flush,
            weight = FontWeight.Normal,
            style = Type.lead,
        ) {
            operate {
                m.forget(id)
                close()
            }
        }
    }
}

internal fun githubStatusText(gh: JSONObject?): String =
    when {
        gh == null -> "確認中…"
        !gh.optBoolean("installed") -> "PC に gh が入っていません"
        gh.optBoolean("loggedIn") -> "${gh.str("login").ifEmpty { "GitHub" }} でログイン中"
        else -> "ログインしていません。PC で実行してください"
    }

internal fun authResultText(flow: JSONObject): String =
    when (flow.str("status")) {
        "succeeded" ->
            if (flow.optBoolean("refreshRequired"))
                "認証情報は保存されました。反映するには PC の Pi で /reload または再起動してください。再入力は不要です。"
            else "ログインしました"
        "failed" ->
            if (flow.str("error") == "storage_failed") "認証情報が保存されたか不明です。再入力する前にプロバイダーの一覧を確認してください。"
            else "ログインできませんでした"
        "expired" -> "期限が切れました"
        else -> "ログイン操作は終了しました"
    }

@Composable
private fun ProviderPage(m: SessionModel, id: String, close: () -> Unit) {
    val rows = m.rowsFor(id)
    if (rows.hostValue("connection") != "\"connected\"") {
        Page("プロバイダーのログイン", close) { Hint("PC に接続していません。再接続後にログイン状態を読み直します。") }
        return
    }
    var retry by remember { mutableIntStateOf(0) }
    key(id, rows.connectionGeneration, retry) { OnlineProviderPage(m, id, close) { retry++ } }
}

@Composable
private fun OnlineProviderPage(m: SessionModel, id: String, close: () -> Unit, reload: () -> Unit) {
    var session by remember { mutableStateOf<LoginSession?>(null) }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    LaunchedEffect(id) {
        checked({ error = it }) {
            m.withLogin(id) { login ->
                try {
                    session = login
                    awaitCancellation()
                } finally {
                    session = null
                }
            }
        }
    }
    val login = session
    val groups = m.rowsFor(id).groups
    val providers =
        groups["provider"]
            .orEmpty()
            .find { it.id == login?.target?.instance }
            ?.let { JSONArray(it.json).objects() }
            .orEmpty()
    val flow =
        groups["auth"]
            .orEmpty()
            .find { it.id == login?.target?.instance && it.json != "null" }
            ?.let { JSONObject(it.json) }
    val active = flow?.str("status") in listOf("input", "working", "saving")
    LaunchedEffect(login, flow?.str("id")) {
        if (login != null && flow != null)
            checked({ error = it }) {
                if (active) {
                    while (login.followOnce()) {
                        yield()
                    }
                    login.request(RcMethod.AUTH_LIST)
                }
            }
    }
    fun send(method: RcMethod, body: JSONObject) {
        if (login == null || busy) return
        busy = true
        error = ""
        scope.launch {
            try {
                checked({ error = it }) {
                    val result = JSONObject(login.request(method, body))
                    message =
                        if (method == RcMethod.AUTH_LOGOUT && result.optBoolean("refreshRequired"))
                            "保存した認証情報を削除しました。反映するには PC の Pi で /reload または再起動してください。"
                        else ""
                    login.request(RcMethod.AUTH_LIST)
                }
            } finally {
                busy = false
            }
        }
    }
    Page("プロバイダーのログイン", close) {
        SheetError(error)
        if (message.isNotEmpty()) Hint(message)
        if (
            error.isNotEmpty() ||
                login != null && (providers.isEmpty() || flow?.str("error") == "storage_failed")
        )
            TextAction("再読み込み", reload, enabled = !busy)
        if (login == null && error.isEmpty()) Hint("専用の Pi を開いています…")
        if (active && flow != null) {
            Label(flow.str("provider"))
            Hint(
                when (flow.str("status")) {
                    "input" -> "入力を待っています"
                    "saving" -> "保存しています…"
                    else -> "ログイン中…"
                }
            )
            flow.optJSONObject("authorization")?.let {
                External("ブラウザで認証", it.getString("url"))
                if (it.str("userCode").isNotEmpty()) CommandPill(it.str("userCode"))
            }
            flow.optJSONObject("prompt")?.let { p ->
                var value by remember(flow.str("id"), p.str("id")) { mutableStateOf("") }
                OutlinedTextField(
                    value,
                    { value = it },
                    label = { Text(if (p.str("kind") == "api_key") "API キー" else "認証コード") },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                )
                TextAction(
                    "送信",
                    {
                        send(
                            RcMethod.AUTH_RESPOND,
                            JSONObject()
                                .put("flowId", flow.str("id"))
                                .put("promptId", p.str("id"))
                                .put("value", value),
                        )
                        value = ""
                    },
                    enabled = !busy && value.isNotBlank(),
                )
            }
            TextAction(
                "キャンセル",
                { send(RcMethod.AUTH_CANCEL, JSONObject().put("flowId", flow.str("id"))) },
                enabled = !busy,
            )
        } else {
            if (flow != null) Hint(authResultText(flow))
            providers.forEach { p ->
                Label(p.str("id"))
                Hint(if (p.optBoolean("configured")) "設定済み" else "未設定")
                p.optJSONArray("methods").strings().forEach { method ->
                    TextAction(
                        if (method == "oauth") "ブラウザでログイン" else "API キーを設定",
                        {
                            send(
                                RcMethod.AUTH_BEGIN,
                                JSONObject().put("provider", p.str("id")).put("method", method),
                            )
                        },
                        enabled = !busy,
                    )
                }
                if (login != null && p.str("stored").isNotEmpty()) {
                    val provider = p.str("id")
                    ConfirmAction(
                        login to provider,
                        "ログアウト",
                        "PC に保存した $provider の認証を削除します。",
                        enabled = !busy,
                    ) {
                        send(RcMethod.AUTH_LOGOUT, JSONObject().put("provider", provider))
                    }
                }
            }
        }
    }
}
