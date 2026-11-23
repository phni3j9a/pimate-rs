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
import androidx.compose.foundation.selection.selectableGroup
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
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.*
import uniffi.pimate_ffi.Host

class MainActivity : ComponentActivity() {
    private val model by viewModels<SessionModel>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val prefs = getSharedPreferences("display", MODE_PRIVATE)
        setContent {
            var scale by remember { mutableFloatStateOf(prefs.getFloat("scale", 1f)) }
            PiTheme(scale) {
                Screen(model, scale) {
                    scale = it
                    prefs.edit().putFloat("scale", it).apply()
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Screen(m: SessionModel, scale: Float, setScale: (Float) -> Unit) {
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    var pairing by rememberSaveable { mutableStateOf(false) }
    var sheet by rememberSaveable { mutableStateOf("") }
    val host = m.host
    val showPair = pairing || host == null
    BackHandler(pairing && host != null) { pairing = false }
    BackHandler(drawer.isOpen) { scope.launch { drawer.close() } }
    LaunchedEffect(m.openList) {
        if (m.openList) {
            pairing = false
            drawer.open()
            m.listed()
        }
    }
    ModalNavigationDrawer(
        drawerState = drawer,
        gesturesEnabled = !showPair,
        scrimColor = pi.scrim,
        drawerContent = {
            ModalDrawerSheet(
                Modifier.fillMaxWidth(.85f).widthIn(max = 330.dp),
                drawerShape = RectangleShape,
                drawerContainerColor = pi.background,
                drawerTonalElevation = 0.dp,
            ) {
                DrawerContent(
                    m,
                    select = {
                        m.select(it)
                        scope.launch { drawer.close() }
                    },
                    pair = {
                        pairing = true
                        scope.launch { drawer.close() }
                    },
                    open = { sheet = it },
                )
            }
        },
    ) {
        if (showPair) PairScreen(m, if (host != null) ({ pairing = false }) else null)
        else ChatScreen(m) { scope.launch { drawer.open() } }
    }
    if (sheet.isNotEmpty())
        ModalBottomSheet(
            { sheet = "" },
            shape = RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp),
            containerColor = pi.background,
            tonalElevation = 0.dp,
            scrimColor = pi.scrim,
        ) {
            Column(Modifier.padding(start = gutter, end = gutter, bottom = 24.dp)) {
                if (sheet == "new") {
                    Text("新しい Pi", Modifier.semantics { heading() }, style = Type.sheet)
                    Text(
                        "スマホからの起動は準備中です。PC で pi を起動すると、一覧に表示されます。",
                        Modifier.padding(top = 8.dp),
                        pi.textSecondary,
                    )
                } else {
                    Text("設定", Modifier.semantics { heading() }, style = Type.sheet)
                    Row(
                        Modifier.heightIn(min = 52.dp).padding(top = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("文字サイズ", Modifier.weight(1f))
                        // セグメント: 濃い塗りの台に、選択中だけ背景色の面（Tokens）。
                        Row(
                            Modifier.background(pi.surfaceStrong, CircleShape)
                                .padding(3.dp)
                                .selectableGroup()
                        ) {
                            fontScales.forEach { (label, value) ->
                                val on =
                                    fontScales
                                        .minBy { kotlin.math.abs(it.second - scale) }
                                        .second == value
                                Box(
                                    Modifier.clip(CircleShape)
                                        .background(if (on) pi.background else pi.surfaceStrong)
                                        .selectable(on, role = Role.RadioButton) { setScale(value) }
                                        .heightIn(min = 42.dp)
                                        .padding(horizontal = 16.dp),
                                    Alignment.Center,
                                ) {
                                    Text(
                                        label,
                                        color = if (on) pi.text else pi.textSecondary,
                                        style =
                                            Type.caption.copy(
                                                fontWeight =
                                                    if (on) FontWeight.SemiBold
                                                    else FontWeight.Normal
                                            ),
                                    )
                                }
                            }
                        }
                    }
                    TextAction(
                        "マシンを追加（ペアリング）",
                        {
                            sheet = ""
                            pairing = true
                            scope.launch { drawer.close() }
                        },
                        Modifier.padding(top = 4.dp).offset(x = (-14).dp),
                    )
                    Text(
                        "ほかの設定は今後の更新で追加します。",
                        Modifier.padding(top = 4.dp),
                        pi.textSecondary,
                        style = Type.caption,
                    )
                }
            }
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

private fun ago(at: Long, now: Long): String {
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

/**
 * ドロワー（docs/design/html/Drawer.html）: 要対応を最上段、つづいてマシンごと。最下段の左に設定、右に「＋ 新しい Pi」。 接続は 1 本なので、Instance
 * を並べるのは接続中のマシンだけ。ほかのマシンは見出しをタップして切り替える。
 */
@Composable
private fun DrawerContent(
    m: SessionModel,
    select: (String) -> Unit,
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
    val host = m.host
    val state = m.rows.groups["host"].orEmpty().associate { it.id to it.json }
    val all =
        remember(m.rows.groups["instance"]) {
            m.rows.groups["instance"].orEmpty().map { it.id to JSONObject(it.json) }
        }
    val shown =
        all.filter { (_, o) ->
            query.isBlank() ||
                title(o.optJSONObject("session"), o.str("cwd")).contains(query.trim(), true) ||
                o.str("cwd").contains(query.trim(), true)
        }
    val question =
        m.rows.groups["question"].orEmpty().lastOrNull()?.let { JSONObject(it.json).str("title") }
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
            val (waiting, rest) = shown.partition { it.second.str("section") == "waiting" }
            if (waiting.isNotEmpty()) {
                item("attention") { GroupHeader(Icons.Bell, "要対応", pi.waitingText) }
                items(waiting, key = { "w:${it.first}" }) { (id, o) ->
                    val asks = o.str("status") == "waiting_input"
                    InstanceRow(
                        o,
                        id == m.selected,
                        when {
                            !asks -> "サブエージェントの確認が必要です"
                            id == m.selected && question != null -> "入力待ち: $question"
                            else -> "入力待ち: Pi が回答を待っています"
                        },
                        ago(o.optLong("updatedAt"), now),
                    ) {
                        select(id)
                    }
                }
            }
            if (host != null) {
                item("host") {
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
                            else -> {
                                Spinner(11.dp)
                                Text("接続しています", color = pi.textSecondary, style = Type.meta)
                            }
                        }
                    }
                }
                val (bots, normal) = rest.partition { it.second.str("section") == "bot" }
                items(normal, key = { "i:${it.first}" }) { (id, o) ->
                    InstanceRow(o, id == m.selected, null, ago(o.optLong("updatedAt"), now)) {
                        select(id)
                    }
                }
                if (bots.isNotEmpty())
                    item("bots") {
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
                items(bots, key = { "b:${it.first}" }) { (id, o) ->
                    InstanceRow(o, id == m.selected, null, ago(o.optLong("updatedAt"), now)) {
                        select(id)
                    }
                }
                if (shown.isEmpty())
                    item("empty") {
                        Text(
                            when {
                                state["connection"] == "\"offline\"" -> "PC に接続できません。再接続を試みています。"
                                state["listKnown"] != "true" -> "読み込み中…"
                                all.isNotEmpty() -> "一致する Pi はありません"
                                else -> "起動中の Pi はありません。PC で pi を起動すると、ここに表示されます。"
                            },
                            Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
                            pi.textSecondary,
                            style = Type.caption,
                        )
                    }
            }
            items(m.hosts.filter { it.id != host?.id }, key = { "h:${it.id}" }) { other ->
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
private fun InstanceRow(
    o: JSONObject,
    selected: Boolean,
    attention: String?,
    right: String,
    onClick: () -> Unit,
) {
    val agents = o.optJSONObject("subagents")
    val active = (agents?.optLong("active") ?: 0) - (agents?.optLong("waiting") ?: 0)
    // 印と右端は親の状態。子（サブエージェント）が動いていることは、パスの後ろの補足で示す。
    val working = o.str("status") == "running"
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
