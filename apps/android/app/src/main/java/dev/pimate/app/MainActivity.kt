package dev.pimate.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.*
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.min
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.*
import uniffi.pimate_ffi.Host

class MainActivity : ComponentActivity() {
    private val model by viewModels<SessionModel>()
    private val chat by viewModels<ChatUi>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val settings = model.settings
            PiTheme(settings.scale, settings.theme) {
                val dark = darkPalette
                SideEffect {
                    androidx.core.view.WindowCompat.getInsetsController(window, window.decorView)
                        .run {
                            isAppearanceLightStatusBars = !dark
                            isAppearanceLightNavigationBars = !dark
                        }
                }
                Screen(model, chat)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Screen(m: SessionModel, chat: ChatUi) {
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val pairing = m.pairing
    var sheet by rememberSaveable { mutableStateOf("") }
    var machine by rememberSaveable { mutableStateOf("") }
    var first by rememberSaveable { mutableStateOf("") }
    val host = m.host
    val showPair = pairing || host == null
    BackHandler(pairing && host != null) { m.showPairing(false) }
    BackHandler(drawer.isOpen) { scope.launch { drawer.close() } }
    // 再接続をやめた接続の立て直し: 再ペアリングの画面へ。バージョン不一致は更新後につなぎ直す。
    val recover = { stop: Stop ->
        if (stop.repair) m.showPairing(true) else host?.let(m::switchHost)
        scope.launch { drawer.close() }
        Unit
    }
    LaunchedEffect(m.openList) {
        if (m.openList) {
            m.showPairing(false)
            if (m.pairedHostId != null) {
                first = m.pairedHostId!!
                m.consumePairedHost()
            } else drawer.open()
            m.listed()
        }
    }
    ModalNavigationDrawer(
        drawerState = drawer,
        gesturesEnabled = !showPair,
        scrimColor = pi.scrim,
        drawerContent = {
            DrawerSheet {
                DrawerContent(
                    m,
                    select = { h, id ->
                        m.select(h, id)
                        scope.launch { drawer.close() }
                    },
                    pair = {
                        m.showPairing(true)
                        scope.launch { drawer.close() }
                    },
                    open = {
                        sheet = it
                        scope.launch { drawer.close() }
                    },
                )
            }
        },
    ) {
        if (showPair) PairScreen(m, if (host != null) ({ m.showPairing(false) }) else null)
        else key(host?.id) { ChatScreen(m, chat, { scope.launch { drawer.open() } }, recover) }
    }
    val firstHost = m.hosts.find { it.id == first }
    if (!showPair && sheet.isEmpty() && firstHost != null)
        FirstPage(
            m,
            firstHost,
            {
                first = ""
                scope.launch { drawer.open() }
            },
            { sheet = "new" },
            {
                first = ""
                m.select(firstHost, it)
            },
        )
    when (sheet) {
        "settings" ->
            SettingsPage(
                m,
                { sheet = "" },
                {
                    machine = it
                    sheet = "machine"
                },
                {
                    sheet = ""
                    m.showPairing(true)
                },
            )
        "machine" ->
            m.hosts
                .find { it.id == machine }
                ?.let {
                    key(it.id, it.deviceId) {
                        MachinePage(m, it, { sheet = "settings" }) {
                            sheet = ""
                            m.showPairing(true)
                        }
                    }
                }
        "new" ->
            NewPage(m, firstHost ?: host, { sheet = "" }) { h, id ->
                first = ""
                sheet = ""
                m.select(h, id)
            }
    }
}

/** ドロワーの面。幅は画面の 85%、広い画面でも 330dp まで。 */
@Composable
internal fun DrawerSheet(content: @Composable ColumnScope.() -> Unit) {
    BoxWithConstraints {
        ModalDrawerSheet(
            Modifier.width(min(maxWidth * .85f, 330.dp)),
            drawerShape = RectangleShape,
            drawerContainerColor = pi.background,
            drawerTonalElevation = 0.dp,
            content = content,
        )
    }
}

private fun basename(path: String) = path.trimEnd('/').substringAfterLast('/').ifEmpty { path }

/** `/home/me/code/demo` → `~/code/demo`。PC のホームは分からないので、よくある配置だけ縮める。 */
internal fun tilde(path: String) =
    Regex("^(/home/[^/]+|/Users/[^/]+|/root)(/.*)?$").find(path)?.let { "~" + it.groupValues[2] }
        ?: path

/** 一覧と見出しの題: セッション名、なければフォルダ名。 */
internal fun title(session: JSONObject?, cwd: String) =
    session?.str("name")?.trim().orEmpty().ifEmpty { basename(cwd) }

internal fun ago(at: Long, now: Long): String {
    val min = (now - at).coerceAtLeast(0) / 60_000
    return when {
        min < 1 -> "今"
        min < 60 -> "${min}分"
        min < 1440 -> "${min / 60}時間"
        min < 2880 -> "昨日"
        min < 10080 -> "${min / 1440}日"
        else -> java.text.SimpleDateFormat("M/d", java.util.Locale.JAPAN).format(java.util.Date(at))
    }
}

@Composable
private fun DrawerContent(
    m: SessionModel,
    select: (Host, String) -> Unit,
    pair: () -> Unit,
    open: (String) -> Unit,
) {
    var query by rememberSaveable { mutableStateOf("") }
    val now by
        produceState(System.currentTimeMillis()) {
            while (true) {
                delay(30_000)
                value = System.currentTimeMillis()
            }
        }
    val shown =
        m.hosts.flatMap { h ->
            val instances = m.rowsFor(h.id).groups["instance"]
            remember(h, instances, query) {
                instances
                    .orEmpty()
                    .map { Triple(h, it.id, JSONObject(it.json)) }
                    .filter { (_, _, o) ->
                        query.isBlank() ||
                            title(o.optJSONObject("session"), o.str("cwd"))
                                .contains(query.trim(), true) ||
                            o.str("cwd").contains(query.trim(), true)
                    }
            }
        }
    Column(Modifier.fillMaxSize().padding(horizontal = 12.dp)) {
        Row(
            Modifier.padding(start = 2.dp, top = 6.dp, end = 2.dp)
                .fillMaxWidth()
                .heightIn(min = tap)
                .background(pi.surface, CircleShape)
                .padding(horizontal = 14.dp),
            Arrangement.spacedBy(8.dp),
            Alignment.CenterVertically,
        ) {
            Icon(Icons.Search, null, Modifier.size(16.dp), pi.textSecondary)
            BasicTextField(
                query,
                { query = it },
                Modifier.weight(1f).semantics { contentDescription = "Instance・フォルダを検索" },
                textStyle = Type.body.copy(color = pi.text),
                singleLine = true,
                cursorBrush = SolidColor(pi.accent),
                decorationBox = { inner ->
                    if (query.isEmpty())
                        Text("Instance・フォルダを検索", color = pi.textSecondary, maxLines = 1)
                    inner()
                },
            )
        }
        LazyColumn(Modifier.weight(1f).padding(top = 4.dp)) {
            val (waiting, rest) = shown.partition { it.third.str("section") == "waiting" }
            if (waiting.isNotEmpty()) {
                item("attention") { GroupHeader(Icons.Bell, "要対応", pi.waitingText) }
                items(
                    waiting.sortedByDescending { it.third.optLong("updatedAt") },
                    key = { "w:${it.first.id}:${it.second}" },
                ) { (h, id, o) ->
                    val asks = o.str("status") == "waiting_input"
                    InstanceRow(
                        o,
                        h.id == m.host?.id && id == m.selected,
                        when {
                            o.has("failureText") -> "失敗: ${o.str("failureText")}"
                            o.has("questionTitle") -> "入力待ち: ${o.str("questionTitle")}"
                            h.id == m.host?.id &&
                                id == m.selected &&
                                !m.rows.groups["question"].isNullOrEmpty() ->
                                "入力待ち: ${JSONObject(m.rows.groups.getValue("question").last().json).str("title")}"
                            !asks -> "サブエージェントの確認が必要です"
                            else -> "入力待ち: Pi が回答を待っています"
                        },
                        ago(o.optLong("updatedAt"), now),
                        machine = h.name,
                    ) {
                        select(h, id)
                    }
                }
            }
            for (host in m.hosts.filterNot { it.needsPairing }) {
                val groups = m.rowsFor(host.id).groups
                val state = groups["host"].orEmpty().associate { it.id to it.json }
                val stop = stopped(state["connection"])
                item("host:${host.id}") {
                    val connection = state["connection"]
                    GroupHeader(
                        if (state["info"]?.contains("\"darwin\"") == true) Icons.Laptop
                        else Icons.Server,
                        host.name,
                        pi.textSecondary,
                    ) {
                        when (connection) {
                            "\"connected\"" -> {
                                Dot(pi.done)
                                Text("接続中", color = pi.textSecondary, style = Type.meta)
                            }
                            "\"offline\"" -> Text("オフライン", color = pi.failedText, style = Type.meta)
                            else ->
                                if (stop != null) {
                                    Dot(pi.failed)
                                    Text(stop.label, color = pi.failedText, style = Type.meta)
                                } else {
                                    Spinner(11.dp)
                                    Text("接続しています", color = pi.textSecondary, style = Type.meta)
                                }
                        }
                    }
                }
                val (bots, normal) =
                    rest
                        .filter { it.first.id == host.id }
                        .partition { it.third.str("section") == "bot" }
                items(normal, key = { "i:${it.first.id}:${it.second}" }) { (h, id, o) ->
                    InstanceRow(
                        o,
                        h.id == m.host?.id && id == m.selected,
                        null,
                        ago(o.optLong("updatedAt"), now),
                    ) {
                        select(h, id)
                    }
                }
                if (bots.isNotEmpty())
                    item("bots:${host.id}") {
                        Text(
                            "Grok Bot",
                            Modifier.padding(start = 36.dp, top = 10.dp, bottom = 2.dp).semantics {
                                heading()
                            },
                            pi.textSecondary,
                            style = Type.meta,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                items(bots, key = { "b:${it.first.id}:${it.second}" }) { (h, id, o) ->
                    InstanceRow(
                        o,
                        h.id == m.host?.id && id == m.selected,
                        null,
                        ago(o.optLong("updatedAt"), now),
                    ) {
                        select(h, id)
                    }
                }
                if (stop != null)
                    item("stop:${host.id}") {
                        Column(Modifier.padding(start = 12.dp, top = 10.dp, end = 12.dp)) {
                            Text(
                                stop.detail,
                                color = pi.textSecondary,
                                style = Type.caption.copy(lineBreak = Type.phrase),
                            )
                            TextAction(
                                stop.action,
                                { if (stop.repair) pair() else m.switchHost(host) },
                                Modifier.offset(x = (-14).dp),
                                enabled = !m.busy,
                            )
                        }
                    }
                else if (shown.none { it.first.id == host.id })
                    item("empty:${host.id}") {
                        Text(
                            when {
                                state["connection"] == "\"offline\"" -> "PC に接続できません。再接続を試みています。"
                                state["listKnown"] != "true" -> "読み込み中…"
                                !groups["instance"].isNullOrEmpty() -> "一致する Pi はありません"
                                else -> "起動中の Pi はありません。PC で pi を起動すると、ここに表示されます。"
                            },
                            Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                            pi.textSecondary,
                            style = Type.caption,
                        )
                    }
            }
            items(m.hosts.filter { it.needsPairing }, key = { "h:${it.id}" }) { other ->
                OtherHost(other, !m.busy) {
                    if (other.needsPairing) pair() else m.switchHost(other)
                }
            }
        }
        if (m.error.isNotEmpty())
            Text(
                m.error,
                Modifier.padding(horizontal = 12.dp, vertical = 4.dp).semantics {
                    liveRegion = LiveRegionMode.Polite
                },
                pi.failedText,
                style = Type.caption,
            )
        Row(
            Modifier.fillMaxWidth().padding(start = 2.dp, top = 8.dp, end = 4.dp, bottom = 16.dp),
            Arrangement.SpaceBetween,
            Alignment.CenterVertically,
        ) {
            IconAction(Icons.Gear, "設定", { open("settings") })
            PrimaryButton(
                "新しい Pi",
                { open("new") },
                Modifier.semantics { contentDescription = "新しい Pi を起動" },
                height = 44.dp,
                icon = Icons.Plus,
            )
        }
    }
}

@Composable
private fun GroupHeader(
    icon: ImageVector,
    name: String,
    color: androidx.compose.ui.graphics.Color,
    trailing: @Composable RowScope.() -> Unit = {},
) {
    Row(
        Modifier.padding(top = 22.dp)
            .fillMaxWidth()
            .heightIn(min = 28.dp)
            .padding(horizontal = 12.dp),
        Arrangement.spacedBy(6.dp),
        Alignment.CenterVertically,
    ) {
        Icon(icon, null, Modifier.size(15.dp), color)
        Text(
            name,
            Modifier.weight(1f).semantics { heading() },
            color,
            style = Type.caption,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Row(
            Modifier.semantics(mergeDescendants = true) {},
            Arrangement.spacedBy(5.dp),
            Alignment.CenterVertically,
            trailing,
        )
    }
}

/** 接続していないマシン。接続は 1 本なので、タップで切り替える（再ペアリングが必要ならその画面へ）。 */
@Composable
private fun OtherHost(host: Host, enabled: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.padding(top = 22.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .clickable(enabled, role = Role.Button, onClick = onClick)
            .heightIn(min = 52.dp)
            .padding(horizontal = 12.dp),
        Arrangement.spacedBy(6.dp),
        Alignment.CenterVertically,
    ) {
        Icon(Icons.Server, null, Modifier.size(15.dp), pi.textSecondary)
        Text(
            host.name,
            Modifier.weight(1f),
            pi.textSecondary,
            style = Type.caption,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            if (host.needsPairing) "再ペアリングが必要" else "切り替える",
            color = if (host.needsPairing) pi.failedText else pi.accentText,
            style = Type.meta,
        )
    }
}

/** Instance の行（60dp 以上）。状態は印の色だけでなく、文字（入力待ち・作業中・経過）でも示す。 */
@Composable
internal fun InstanceRow(
    o: JSONObject,
    selected: Boolean,
    attention: String?,
    right: String,
    machine: String? = null,
    onClick: () -> Unit,
) {
    val agents = o.optJSONObject("subagents")
    val active = (agents?.optLong("active") ?: 0) - (agents?.optLong("waiting") ?: 0)
    val working = o.optBoolean("working")
    Row(
        Modifier.padding(top = 2.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(if (selected) pi.selected else pi.background)
            .selectable(selected, role = Role.Button, onClick = onClick)
            .heightIn(min = 60.dp)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        Arrangement.spacedBy(10.dp),
    ) {
        MarkColumn(21.dp) {
            when {
                attention != null -> Dot(pi.waiting, 8.dp)
                working -> Spinner()
                else -> Dot(pi.textSecondary, 5.dp, Modifier.alpha(.5f))
            }
        }
        Column(Modifier.weight(1f)) {
            Text(
                title(o.optJSONObject("session"), o.str("cwd")),
                fontWeight = if (attention != null) FontWeight.SemiBold else FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (machine != null)
                Text(
                    machine,
                    color = pi.textSecondary,
                    style = Type.meta,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            if (attention != null)
                Text(
                    attention,
                    Modifier.padding(top = 1.dp),
                    pi.waitingText,
                    style = Type.caption,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            else
                Row(Modifier.padding(top = 1.dp), Arrangement.spacedBy(6.dp)) {
                    Text(
                        tilde(o.str("cwd")),
                        Modifier.weight(1f, false),
                        pi.textSecondary,
                        style = Type.meta.copy(fontFamily = Mono),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    val note =
                        when {
                            active > 0 -> "· サブエージェント $active 実行中"
                            o.str("origin") == "mobile" -> "· スマホで起動"
                            else -> ""
                        }
                    if (note.isNotEmpty())
                        Text(note, color = pi.textSecondary, style = Type.meta, maxLines = 1)
                }
        }
        Text(
            if (working && attention == null) "作業中" else right,
            Modifier.padding(top = 2.dp),
            pi.textSecondary,
            style = Type.meta,
        )
    }
}
