package dev.pimate.app

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.*
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
            info().str("platform").ifEmpty { null },
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
            Label("マシン")
            m.hosts.forEach {
                MenuRow(
                    it.name,
                    if (h == it) "選択中" else m.rowsFor(it.id).summary(),
                    !busy && !it.needsPairing,
                ) {
                    hostId = it.id
                }
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
    var result by remember { mutableStateOf<AppUpdate?>(null) }
    var checking by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    Page("設定", close) {
        Label("マシン")
        m.hosts.forEach {
            MenuRow(it.name, m.rowsFor(it.id).summary(), true, next = true) { machine(it.id) }
        }
        TextAction("マシンを追加（ペアリング）", pair)
        Label("新しい Pi の既定")
        Toggle("質問ツール（pimate_ask）", s.askTool, s::chooseAskTool)
        Label("表示")
        Text("テーマ")
        Segment(listOf("SYSTEM" to "システム", "LIGHT" to "ライト", "DARK" to "ダーク"), s.theme.name) {
            s.chooseTheme(ThemeMode.valueOf(it))
        }
        Text("文字サイズ")
        Segment(fontScales.map { it.second.toString() to it.first }, s.scale.toString()) {
            s.chooseScale(it.toFloat())
        }
        Label("情報")
        MenuRow("PiMate $version", if (checking) "照会中…" else "更新を確認", !checking) {
            checking = true
            scope.launch {
                try {
                    result = checkAppUpdate(version)
                } finally {
                    checking = false
                }
            }
        }
        when (val r = result) {
            AppUpdate.Latest -> Hint("最新です")
            AppUpdate.Unknown -> Hint("更新を確認できませんでした")
            is AppUpdate.Available -> External("${r.version} に更新できます", r.url)
            null -> Unit
        }
        External("GitHub", REPO)
        External("不具合を報告", REPORT_ISSUE)
    }
}

@Composable
internal fun MachinePage(m: SessionModel, host: Host, close: () -> Unit) {
    val id = host.id
    val rows = m.rowsFor(id)
    val caps = rows.info()
    val online = rows.hostValue("connection") == "\"connected\""
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
    var confirm by remember { mutableStateOf("") }
    var device by remember { mutableStateOf<JSONObject?>(null) }
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
    Page(host.name, close) {
        Label("状態")
        Text(rows.summary())
        Hint(host.address)
        devices
            ?.find { it.optBoolean("current") }
            ?.optLong("createdAt")
            ?.takeIf { it > 0 }
            ?.let {
                Hint(android.text.format.DateFormat.format("yyyy/MM/dd", it).toString() + " にペアリング")
            }
        Label("PC の PiMate")
        val current = check?.str("current").orEmpty().ifEmpty { caps.str("version") }
        Text("現在 ${current.ifEmpty { "不明" }}")
        val latest = check?.str("latest").orEmpty()
        when {
            check?.str("reason") == "not_package" -> Hint("開発版は PC で更新してください")
            latest == "" -> Hint("最新版は未確認です")
            latest == current -> Hint("最新です")
            check?.optBoolean("canUpdate") == true ->
                TextAction("$latest に更新", { confirm = "更新" }, enabled = !busy)
            else -> {
                Hint("PC のターミナルで更新してください")
                CommandPill("pi update npm:@phni3j9a/pimate")
            }
        }
        if (online && caps.optBoolean("hostRestart"))
            TextAction("Host を再起動", { confirm = "Host を再起動" }, enabled = !busy)
        Label("プロバイダー")
        MenuRow("プロバイダーのログイン", "", online && caps.optBoolean("authSession") && !busy, next = true) {
            login = true
        }
        Label("GitHub")
        Text(
            when {
                gh == null -> "ログイン状態は未確認です"
                gh?.optBoolean("loggedIn") == true -> "${gh!!.str("login")} でログイン中"
                else -> "PC で gh auth login を実行してください"
            }
        )
        Label("ペアリング済みの端末")
        if (devices == null) Hint("端末一覧は未確認です")
        devices.orEmpty().forEach { d ->
            Text(d.str("name"))
            if (d.optLong("lastSeenAt") > 0)
                Hint(
                    "最終接続 " +
                        android.text.format.DateFormat.format("yyyy/MM/dd", d.optLong("lastSeenAt"))
                )
            if (d.optBoolean("current")) Hint("このスマホ")
            else
                TextAction(
                    "取り消し",
                    {
                        device = d
                        confirm = "端末の取り消し"
                    },
                    enabled = !busy,
                    color = pi.failedText,
                )
        }
        SheetError(error)
        if (note.isNotEmpty()) Hint(note)
        TextAction("状態を再読み込み", { retry++ }, enabled = online && !busy)
        TextAction("このマシンを削除", { confirm = "このマシンを削除" }, enabled = !busy, color = pi.failedText)
    }
    if (confirm.isNotEmpty())
        Confirm(
            confirm,
            when (confirm) {
                "端末の取り消し" -> "${device?.str("name")} はこの PC に接続できなくなります。"
                "このマシンを削除" -> "このスマホに保存した ${host.name} の登録と下書きを削除します。"
                else -> "${host.name} との接続が一時的に切れる場合があります。"
            },
            { confirm = "" },
        ) {
            val action = confirm
            confirm = ""
            busy = true
            note = ""
            scope.launch {
                try {
                    checked({ error = it }) {
                        when (action) {
                            "このマシンを削除" -> {
                                m.forget(id)
                                close()
                            }
                            "端末の取り消し" -> {
                                request(Operation.Revoke(device!!.getString("deviceId")))
                                retry++
                                note = "登録を取り消しました"
                            }
                            "更新" -> {
                                val r = request(Operation.Update)
                                note = "${r.getString("to")} に更新しました"
                                retry++
                            }
                            else -> {
                                request(Operation.Restart)
                                note = "再起動を受け付けました"
                            }
                        }
                    }
                } finally {
                    busy = false
                }
            }
        }
}

@Composable
private fun ProviderPage(m: SessionModel, id: String, close: () -> Unit) {
    var session by remember { mutableStateOf<LoginSession?>(null) }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var retry by remember { mutableIntStateOf(0) }
    var logout by remember { mutableStateOf("") }
    val scope = rememberCoroutineScope()
    LaunchedEffect(id, retry) {
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
                    login.request(method, body)
                    login.request(RcMethod.AUTH_LIST)
                }
            } finally {
                busy = false
            }
        }
    }
    Page("プロバイダーのログイン", close) {
        SheetError(error)
        if (error.isNotEmpty())
            TextAction(
                "再読み込み",
                {
                    error = ""
                    retry++
                },
            )
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
            if (flow != null)
                Hint(
                    when (flow.str("status")) {
                        "succeeded" -> "ログインしました"
                        "failed" -> "ログインできませんでした"
                        "expired" -> "期限が切れました"
                        else -> "ログイン操作は終了しました"
                    }
                )
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
                if (p.str("stored").isNotEmpty())
                    TextAction(
                        "ログアウト",
                        { logout = p.str("id") },
                        enabled = !busy,
                        color = pi.failedText,
                    )
            }
        }
    }
    if (logout.isNotEmpty())
        Confirm("ログアウト", "PC に保存した $logout の認証を削除します。", { logout = "" }) {
            send(RcMethod.AUTH_LOGOUT, JSONObject().put("provider", logout))
            logout = ""
        }
}
