package dev.pimate.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.*
import androidx.activity.viewModels
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.journeyapps.barcodescanner.*
import kotlinx.coroutines.launch
import org.json.*
import uniffi.pimate_ffi.*

class MainActivity : ComponentActivity() {
    private val model by viewModels<SessionModel>()
    override fun onCreate(savedInstanceState: Bundle?) { super.onCreate(savedInstanceState); initLogging(); enableEdgeToEdge()
        val prefs = getSharedPreferences("display", MODE_PRIVATE)
        setContent { var scale by remember { mutableFloatStateOf(prefs.getFloat("scale",1f)) }; PiTheme(scale) { Screen(model) { scale = it; prefs.edit().putFloat("scale",it).apply() } } }
    }
}
@Composable private fun Screen(m: SessionModel, scale: (Float)->Unit) {
    val drawer = rememberDrawerState(DrawerValue.Closed); val scope = rememberCoroutineScope()
    var pairing by rememberSaveable { mutableStateOf(false) }; val host = m.host
    val showPair = pairing || host == null
    BackHandler(pairing && host != null) { pairing = false }
    LaunchedEffect(host) { if (host != null) { pairing = false; drawer.open() } }
    Surface { ModalNavigationDrawer(drawerState = drawer, gesturesEnabled = !showPair, drawerContent = {
        ModalDrawerSheet { LazyColumn(Modifier.padding(gutter)) {
            item { Text(host?.name ?: "マシン",style = MaterialTheme.typography.titleMedium) }
            items(m.rows.groups["instance"].orEmpty(), key = { it.id }) { row -> TextButton(onClick = { m.select(row.id); scope.launch { drawer.close() } }) { Text(JSONObject(row.json).optString("cwd",row.id)) } }
            item { TextButton(onClick = { pairing = true; scope.launch { drawer.close() } }) { Text("ペアリング") }; Text("文字サイズ"); Row { listOf("小" to .85f,"標準" to 1f,"大" to 1.2f).forEach { (label,s) -> TextButton({ scale(s) }) { Text(label) } } } }
        } }
    }) { Column(Modifier.fillMaxSize().safeDrawingPadding().imePadding().padding(horizontal = gutter),verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (showPair) PairScreen(m) else {
            TextButton({scope.launch { drawer.open() }}) { Text("Instance 一覧") }
            val groups = m.rows.groups; val entries = groups["entry"].orEmpty(); val stream = groups["stream"].orEmpty().firstOrNull { it.id == "text" }?.let { JSONTokener(it.json).nextValue() as? String }.orEmpty()
            val view = groups["view"].orEmpty().associate { it.id to it.json }; val session = view["session"]?.let { JSONObject(it).optString("id") }; val target = m.selected?.let { if (session == null) null else Target(it,session) }
            val scroll = rememberLazyListState(); LazyColumn(Modifier.weight(1f),state = scroll,verticalArrangement = Arrangement.spacedBy(8.dp)) {
                items(entries,key = { "entry:${it.id}" }) { val obj = JSONObject(it.json); Markdown(obj.optString("text",obj.optString("kind"))) }
                if (stream.isNotEmpty()) item("stream") { Markdown(stream) }; items(groups["question"].orEmpty()) { Text(JSONObject(it.json).optString("title")) }
            }; LaunchedEffect(entries.size,stream.isNotEmpty()) { if (scroll.layoutInfo.totalItemsCount > 0) scroll.scrollToItem(scroll.layoutInfo.totalItemsCount-1) }
            var input by remember(target) { mutableStateOf("") }; OutlinedTextField(input,{input=it},Modifier.fillMaxWidth(),label = { Text("メッセージ") },maxLines=5)
            Row { Button({ val text = input; target?.let { m.operate(Operation.Send(it,text, emptyList())) { if (text == input) input="" } } }, enabled = !m.busy && input.isNotBlank() && target != null) { Text(if(view["agentActive"]=="true") "割り込む" else "送信") }; if(view["agentActive"]=="true" && target!=null) TextButton({m.operate(Operation.Abort(target))}) { Text("停止") } }
        }; if(m.error.isNotEmpty()) Text(m.error,color=MaterialTheme.colorScheme.error)
    } } }
}
@Composable private fun PairScreen(m: SessionModel) {
    var link by remember { mutableStateOf("") }; val scanner = rememberLauncherForActivityResult(ScanContract()) { r -> r.contents?.let { m.pair(it) } }
    Text("PC とペアリング",style = MaterialTheme.typography.titleMedium); OutlinedTextField(link,{ link=it },Modifier.fillMaxWidth(), label={Text("ペアリングリンク")}, singleLine=true)
    Button({ m.pair(link) }, enabled=link.isNotBlank() && !m.busy) { Text("接続") }; TextButton({ scanner.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setBeepEnabled(false).setPrompt("PC の QR コードを読み取る")) },enabled=!m.busy) { Text("QR コードを読み取る") }
}
