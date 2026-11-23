package dev.pimate.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.*
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.json.*
import uniffi.pimate_ffi.initLogging

class MainActivity : ComponentActivity() {
    private val model by viewModels<SessionModel>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        initLogging()
        enableEdgeToEdge()
        val prefs = getSharedPreferences("display", MODE_PRIVATE)
        setContent {
            var scale by remember { mutableFloatStateOf(prefs.getFloat("scale", 1f)) }
            PiTheme(scale) {
                Screen(model) {
                    scale = it
                    prefs.edit().putFloat("scale", it).apply()
                }
            }
        }
    }
}

@Composable
private fun Screen(m: SessionModel, scale: (Float) -> Unit) {
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    var pairing by rememberSaveable { mutableStateOf(false) }
    val host = m.host
    val showPair = pairing || host == null
    BackHandler(pairing && host != null) { pairing = false }
    LaunchedEffect(host) {
        if (host != null) {
            pairing = false
            drawer.open()
        }
    }
    Surface {
        ModalNavigationDrawer(
            drawerState = drawer,
            gesturesEnabled = !showPair,
            drawerContent = {
                ModalDrawerSheet {
                    LazyColumn(Modifier.padding(gutter)) {
                        item {
                            Text(host?.name ?: "マシン", style = MaterialTheme.typography.titleMedium)
                            m.hosts
                                .filter { it.id != host?.id && !it.needsPairing }
                                .forEach { h ->
                                    TextButton({ m.switchHost(h) }, enabled = !m.busy) {
                                        Text(h.name)
                                    }
                                }
                            val status = m.rows.groups["host"]?.find { it.id == "connection" }?.json
                            if (status != null && status != "\"connected\"")
                                Text("接続状態: ${JSONTokener(status).nextValue()}")
                        }
                        items(m.rows.groups["instance"].orEmpty(), key = { it.id }) { row ->
                            TextButton(
                                onClick = {
                                    m.select(row.id)
                                    scope.launch { drawer.close() }
                                }
                            ) {
                                Text(JSONObject(row.json).optString("cwd", row.id))
                            }
                        }
                        item {
                            TextButton(
                                onClick = {
                                    pairing = true
                                    scope.launch { drawer.close() }
                                }
                            ) {
                                Text("ペアリング")
                            }
                            Text("文字サイズ")
                            Row {
                                listOf("小" to .85f, "標準" to 1f, "大" to 1.2f).forEach { (label, s) ->
                                    TextButton({ scale(s) }) { Text(label) }
                                }
                            }
                        }
                    }
                }
            },
        ) {
            Column(
                Modifier.fillMaxSize()
                    .safeDrawingPadding()
                    .imePadding()
                    .padding(horizontal = gutter),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (showPair) PairScreen(m)
                else {
                    TextButton({ scope.launch { drawer.open() } }) { Text("Instance 一覧") }
                    ChatScreen(m)
                }
                if (m.error.isNotEmpty()) Text(m.error, color = MaterialTheme.colorScheme.error)
            }
        }
    }
}
