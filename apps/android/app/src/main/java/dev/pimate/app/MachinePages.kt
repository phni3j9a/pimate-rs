package dev.pimate.app

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
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
import androidx.compose.ui.focus.onFocusChanged
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
        val rows = m.rowsFor(hostId)
        val online = rows.hostValue("connection") == "\"connected\""
        val listKnown = rows.hostValue("listKnown") == "true"
        var models by remember(online, listKnown) { mutableStateOf<JSONObject?>(null) }
        var loadingModels by remember(online, listKnown) { mutableStateOf(online) }
        val split = remember(form.cwd) { splitFolderPath(form.cwd) }
        val parent = split?.parent
        var focused by remember { mutableStateOf(false) }
        val canList = online && rows.info().optBoolean("dirs")
        var dirs by remember(parent, canList) { mutableStateOf(emptyList<String>()) }
        LaunchedEffect(parent, canList) {
            if (parent != null && canList) {
                delay(150)
                checked({ /* Completion is optional; a manually typed path remains usable. */ }) {
                    dirs =
                        JSONObject(m.machineRequest(hostId, Operation.Dirs(parent)))
                            .optJSONArray("dirs")
                            .strings()
                }
            }
        }
        var modelPicker by remember { mutableStateOf(false) }
        var error by remember { mutableStateOf("") }
        var busy by remember { mutableStateOf(false) }
        var outcome by remember { mutableStateOf("") }
        val scope = rememberCoroutineScope()
        LaunchedEffect(hostId, online) {
            folders = null
            if (!online) return@LaunchedEffect
            checked({ error = it }) {
                folders = JSONObject(m.machineRequest(hostId, Operation.Folders))
            }
        }
        LaunchedEffect(hostId, online, listKnown) {
            if (online && listKnown) {
                checked({ error = it }) { models = m.hostModels(hostId) }
                loadingModels = false
            }
        }
        val recent = folders?.optJSONArray("folders").objects()
        fun folder(path: String) =
            form.folder(
                path,
                recent.any {
                    sameFolder(path, it.str("path"), folders?.str("home").orEmpty()) &&
                        it.optBoolean("trusted")
                },
            )
        Page(
            "新しい Pi",
            close,
            centered = true,
            gap = 0.dp,
            footer = {
                if (error.isNotEmpty()) SheetError(error)
                else if (outcome.isNotEmpty()) Hint(outcome)
                else
                    Text(
                        "起動後は PC の pimate attach で同じ Pi に接続できます",
                        Modifier.fillMaxWidth(),
                        color = pi.textSecondary,
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                        style = Type.caption,
                    )
                Spacer(Modifier.height(6.dp))
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
                    enabled =
                        h != null && online && form.cwd.isNotBlank() && !busy && outcome.isEmpty(),
                )
            },
        ) {
            Label("マシン", top = 4.dp)
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
            Label("フォルダ", top = 20.dp, bottom = 8.dp)
            SpawnInput(
                form.cwd,
                ::folder,
                "フォルダのパス",
                "~/code/project",
                !busy,
                path = true,
                modifier = Modifier.onFocusChanged { focused = it.isFocused },
            )
            if (focused && split != null)
                matchFolders(dirs, split.prefix).forEach { name ->
                    MenuRow("$name/", "", !busy) { folder("${split.parent}$name/") }
                }
            if (!online) Hint("このマシンに接続していません")
            else if (folders == null && error.isEmpty()) Hint("フォルダを読み込み中…")
            if (recent.isNotEmpty()) Label("最近使ったフォルダ", top = 14.dp, bottom = 0.dp)
            recent.take(8).forEach {
                val path = it.str("path")
                val shown = shortFolder(path, folders?.str("home").orEmpty())
                Row(
                    Modifier.fillMaxWidth()
                        .clickable(!busy, role = Role.Button) { folder(shown) }
                        .heightIn(min = 44.dp),
                    Arrangement.spacedBy(10.dp),
                    Alignment.CenterVertically,
                ) {
                    Text(
                        shown,
                        Modifier.weight(1f),
                        style = Type.mono,
                        maxLines = 1,
                        overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                    )
                    val running = it.optInt("running")
                    val lastUsed = it.optLong("lastUsed")
                    Hint(
                        if (running > 0) "Pi $running 件実行中"
                        else if (lastUsed > 0) ago(lastUsed, System.currentTimeMillis()) else ""
                    )
                }
            }
            Spacer(Modifier.height(20.dp))
            Toggle(
                "このフォルダを信頼する",
                form.trust,
                "フォルダ内の設定と Extension を読み込みます。オンにすると PC の Pi にも記憶され、次回から確認されません。",
            ) {
                if (!busy) form.trust = it
            }
            Spacer(Modifier.height(20.dp))
            Toggle("質問ツールを使う", form.askTool, "迷ったときに Pi が選択式で質問します（pimate_ask）。スマホからも回答できます。") {
                if (!busy) {
                    form.askTool = it
                    m.settings.chooseAskTool(it)
                }
            }
            val catalog = models?.optJSONArray("models").objects()
            val saved = models?.optJSONObject("default")
            fun label(ref: JSONObject?, thinking: String?): String? =
                ref?.let {
                    val entry = catalog.find { model -> same(it, model) }
                    modelLabel(
                        it.str("id"),
                        entry?.str("name")?.takeIf(String::isNotBlank),
                        thinking.orEmpty(),
                        entry?.optJSONArray("thinkingLevels")?.length(),
                    )
                }
            val defaultLabel = label(saved?.optJSONObject("model"), saved?.str("thinking"))
            val value =
                label(form.model, form.thinking) ?: defaultLabel?.let { "Pi の既定（$it）" } ?: "Pi の既定"
            Spacer(Modifier.height(16.dp))
            ListRow(
                "モデル",
                enabled = online && !busy,
                onClick = { modelPicker = true },
                trailing = {
                    Text(value, color = pi.textSecondary, style = Type.body)
                    Icon(Icons.Right, null, Modifier.size(14.dp), pi.textSecondary)
                },
            )
            if (modelPicker)
                PiSheet({ modelPicker = false }) {
                    Row(
                        Modifier.fillMaxWidth(),
                        Arrangement.SpaceBetween,
                        Alignment.CenterVertically,
                    ) {
                        Box(Modifier.weight(1f)) { SheetTitle("モデル") }
                        TextAction("完了", { modelPicker = false })
                    }
                    val favorites = models?.optJSONArray("scoped").objects()
                    val searchable = catalog.size > 6
                    var query by remember(searchable) { mutableStateOf("") }
                    var all by remember(favorites.isEmpty()) { mutableStateOf(favorites.isEmpty()) }
                    if (favorites.isNotEmpty())
                        Segment(
                            listOf(
                                "favorites" to "お気に入り ${favorites.size}",
                                "all" to "すべて ${catalog.size}",
                            ),
                            if (all) "all" else "favorites",
                        ) {
                            all = it == "all"
                        }
                    if (searchable)
                        SpawnInput(query, { query = it }, "モデルを検索", "モデルを検索", true, path = true)
                    if (query.isBlank())
                        ListRow(
                            "Pi の既定",
                            defaultLabel ?: "PC の Pi の設定に従います",
                            onClick = {
                                form.model = null
                                form.thinking = null
                            },
                        )
                    val shown =
                        catalog.filter { model ->
                            (all || favorites.any { same(it, model) }) &&
                                "${model.str("provider")}/${model.str("id")} ${model.str("name")}"
                                    .contains(query.trim(), true)
                        }
                    if (shown.isEmpty())
                        Hint(
                            if (loadingModels) "モデルを読み込み中…"
                            else if (models == null) "モデル一覧を取得できません。起動時の設定: $value"
                            else "一致するモデルはありません"
                        )
                    shown
                        .groupBy { it.str("provider") }
                        .forEach { (provider, list) ->
                            Label(providerName(provider), top = 12.dp)
                            list.forEach { model ->
                                ListRow(
                                    model.str("name").ifEmpty { model.str("id") },
                                    "$provider/${model.str("id")}",
                                    onClick = {
                                        form.model =
                                            JSONObject()
                                                .put("provider", provider)
                                                .put("id", model.str("id"))
                                        form.thinking =
                                            form.thinking?.takeIf {
                                                it in model.optJSONArray("thinkingLevels").strings()
                                            }
                                    },
                                    trailing = {
                                        if (same(model, form.model))
                                            Icon(
                                                Icons.Check,
                                                "選択中",
                                                Modifier.size(16.dp),
                                                pi.accentText,
                                            )
                                    },
                                )
                            }
                        }
                    val levels =
                        catalog
                            .find { same(it, form.model) }
                            ?.optJSONArray("thinkingLevels")
                            .strings()
                    if (levels.size > 1) {
                        Label("Thinking", top = 16.dp)
                        Segment(levels.map { it to thinkingShort(it) }, form.thinking) {
                            form.thinking = it
                        }
                        TextAction(
                            "Pi の既定のレベル" +
                                saved
                                    ?.str("thinking")
                                    .orEmpty()
                                    .takeIf { it.isNotBlank() }
                                    ?.let { "（$it）" }
                                    .orEmpty(),
                            { form.thinking = null },
                        )
                    }
                }
            Label("最初のメッセージ（任意）", top = 20.dp, bottom = 8.dp)
            SpawnInput(form.message, { form.message = it }, "最初のメッセージ", "起動したらすぐに送るメッセージ", !busy)
        }
    }
}

private fun shortFolder(path: String, home: String) =
    if (home.isNotEmpty() && (path == home || path.startsWith("$home/")))
        "~" + path.removePrefix(home)
    else path

internal fun sameFolder(path: String, known: String, home: String): Boolean {
    fun normalize(s: String) = s.trimEnd('/').ifEmpty { if (s.startsWith('/')) "/" else "" }
    val short = shortFolder(known, home)
    return path.isNotBlank() &&
        (normalize(path) == normalize(known) || normalize(path) == normalize(short))
}

@Composable
private fun SpawnInput(
    value: String,
    change: (String) -> Unit,
    label: String,
    hint: String,
    enabled: Boolean,
    path: Boolean = false,
    modifier: Modifier = Modifier,
) {
    androidx.compose.foundation.text.BasicTextField(
        value,
        change,
        modifier
            .fillMaxWidth()
            .semantics { contentDescription = label }
            .background(pi.surface, if (path) CircleShape else RoundedCornerShape(14.dp))
            .padding(horizontal = 16.dp, vertical = 12.dp),
        enabled = enabled,
        singleLine = path,
        textStyle =
            Type.body.copy(color = pi.text, fontFamily = if (path) Mono else Type.body.fontFamily),
        cursorBrush = androidx.compose.ui.graphics.SolidColor(pi.accent),
        decorationBox = { inner ->
            Box(Modifier.heightIn(min = if (path) 20.dp else 48.dp)) {
                if (value.isEmpty())
                    Text(hint, color = pi.textSecondary, style = if (path) Type.mono else Type.body)
                inner()
            }
        },
    )
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
                if (m.hosts.any { it.needsPairing }) "ペアリングし直す・マシンを追加" else "マシンを追加（ペアリング）",
                color = pi.accentText,
                style = Type.lead,
            )
        }
        Label("新しい Pi の既定", bottom = 0.dp)
        Toggle("質問ツール（pimate_ask）", s.askTool, set = s::chooseAskTool)
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
    // 読み込みに失敗した欄。失敗した欄を「確認中…」のままにしない。
    var unloaded by remember(online) { mutableStateOf(emptySet<String>()) }
    var retry by remember { mutableIntStateOf(0) }
    var error by remember { mutableStateOf("") }
    var note by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()
    suspend fun request(op: Operation) = JSONObject(m.machineRequest(id, op))
    LaunchedEffect(id, online, retry, caps.toString()) {
        error = ""
        unloaded = emptySet()
        // 読み直す間は「確認中…」に戻す（端末の一覧は、取り消しのあとも並びを保つ）。
        check = null
        gh = null
        suspend fun load(part: String, task: suspend () -> Unit) =
            checked(
                {
                    error = it
                    unloaded += part
                },
                task,
            )
        if (online)
            coroutineScope {
                if (caps.optBoolean("pairedDevices"))
                    launch {
                        load("devices") {
                            devices = request(Operation.Devices).getJSONArray("devices").objects()
                        }
                    }
                if (caps.optBoolean("githubStatus"))
                    launch {
                        load("github") {
                            gh = request(Operation.GithubStatus).getJSONObject("status")
                        }
                    }
                if (caps.optBoolean("packageUpdate"))
                    launch { load("update") { check = request(Operation.UpdateCheck) } }
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
                TextAction(
                    "ペアリングし直す",
                    pair,
                    flushEnd,
                    weight = FontWeight.Normal,
                    style = Type.lead,
                )
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
        // 公開版のほうが新しいときだけ案内する（開発中の版が公開版より先でも「更新できます」と出さない）。
        val available = newer(latest, current)
        // Host は公開版を照会できないと、失敗にせず latest のない結果を返す。これも読み直しの対象にする。
        val unchecked = check != null && latest == ""
        val canUpdate =
            online &&
                available &&
                caps.optBoolean("packageUpdate") &&
                check?.optBoolean("canUpdate") == true
        // Host が自分では更新できない（常駐サービスでない・公開パッケージでない）。PC で操作してもらう。
        val manual = check?.str("reason").orEmpty().takeIf { online && available }
        ListRow(
            if (current.isEmpty()) "PiMate" else "現在 $current",
            when {
                !online -> "接続していません"
                !caps.optBoolean("packageUpdate") -> "PC の PiMate を更新すると使えます"
                check == null -> if ("update" in unloaded) "確認できませんでした" else "確認中…"
                unchecked -> "最新版を確認できませんでした"
                available -> "$latest に更新できます"
                else -> "最新です"
            },
            trailing =
                if (!canUpdate) null
                else {
                    {
                        ConfirmAction(
                            host to latest,
                            "更新",
                            "${host.name} を実行時の最新版に更新します（確認時点の最新版: $latest）。接続が一時的に切れる場合があります。実行中の Pi は /reload または再起動まで旧版のままです。",
                            enabled = !busy,
                            title = "PC の PiMate を更新しますか？",
                            modifier = flushEnd,
                            color = pi.accentText,
                            style = Type.lead,
                        ) {
                            operate {
                                val r = request(Operation.Update)
                                note =
                                    if (r.getString("to") == r.getString("from"))
                                        "更新されませんでした（版が固定されている可能性があります）"
                                    else
                                        "${r.getString("to")} に更新しました。実行中の各 Pi で /reload を実行してください。"
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
            ListRow(githubStatusText(status, "github" in unloaded))
            if (status != null && status.optBoolean("installed") && !status.optBoolean("loggedIn"))
                CommandPill("gh auth login", Modifier.padding(bottom = 8.dp))
        }
        if (online && caps.optBoolean("pairedDevices")) {
            Label("ペアリング済みの端末", bottom = 0.dp)
            if (devices == null) Hint(if ("devices" in unloaded) "端末の一覧を確認できませんでした" else "確認中…")
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
        // 読み込みや操作に失敗したとき、公開版を照会できなかったときに出す。
        if (error.isNotEmpty() || unloaded.isNotEmpty() || unchecked)
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

internal fun githubStatusText(gh: JSONObject?, failed: Boolean = false): String =
    when {
        gh == null -> if (failed) "確認できませんでした" else "確認中…"
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
