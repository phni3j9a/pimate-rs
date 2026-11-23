package dev.pimate.app

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.*
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.journeyapps.barcodescanner.*

/**
 * ペアリングの最初の画面（docs/design/html/Pair.html）。準備済みならそのまま QR を読める。 onBack があるのは、接続済みのマシンがあって追加で開いたとき。
 */
@Composable
internal fun PairScreen(m: SessionModel, onBack: (() -> Unit)? = null) {
    // リンクは 1 回限りの秘密なので、保存される状態（SavedState）には載せない。
    var link by remember { mutableStateOf("") }
    var paste by rememberSaveable { mutableStateOf(false) }
    var guide by rememberSaveable { mutableIntStateOf(0) }
    var invalid by remember { mutableStateOf(false) }
    fun submitLink(candidate: String) {
        invalid = uniffi.pimate_ffi.previewPair(candidate) == null
        if (!invalid) m.pair(candidate)
    }
    val scanner =
        rememberLauncherForActivityResult(ScanContract()) { r -> r.contents?.let(::submitLink) }
    if (guide != 0)
        SetupGuide(
            guide,
            { guide = 0 },
            { guide = 2 },
            {
                guide = 0
                scanner.launch(
                    ScanOptions()
                        .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                        .setBeepEnabled(false)
                )
            },
        )
    val code = pi.textCode
    Column(Modifier.fillMaxSize().background(pi.background).safeDrawingPadding()) {
        if (onBack != null)
            Row(
                Modifier.height(52.dp).padding(horizontal = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconAction(Icons.Back, "戻る", onBack, tint = pi.text)
                Text("マシンを追加", Modifier.semantics { heading() }, style = Type.header)
            }
        else Spacer(Modifier.height(36.dp))
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = gutter)
        ) {
            if (onBack == null) {
                // マークは 1024 の版面の中の 920×816（icon-kit）。44dp の枠に同じ比率で置く。
                Image(
                    painterResource(R.drawable.mark),
                    null,
                    Modifier.size(44.dp).padding(horizontal = 2.dp, vertical = 4.5.dp),
                )
                Text(
                    "PC の Pi とつなぐ",
                    Modifier.padding(top = 18.dp).semantics { heading() },
                    style = Type.title,
                )
                Text(
                    "PC で動いている Pi を、このスマホから確認・操作できます。",
                    Modifier.padding(top = 8.dp),
                    pi.textSecondary,
                    style = Type.lead.copy(lineBreak = Type.phrase),
                )
            }
            PiTerminal(Modifier.padding(top = if (onBack == null) 24.dp else 8.dp))
            Column(Modifier.padding(top = 20.dp)) {
                Step(1, "スマホと PC を Tailscale でつなぐ", AnnotatedString("両方で同じアカウントにログイン")) {
                    guide = 1
                }
                Step(
                    2,
                    "PC に PiMate を入れる",
                    buildAnnotatedString {
                        append("Pi で ")
                        withStyle(SpanStyle(fontFamily = Mono, color = code)) {
                            append("/pimate setup")
                        }
                        append(" を実行")
                    },
                ) {
                    guide = 2
                }
                Step(3, "QR コードを読み取る", AnnotatedString("コードは 10 分間、1 回だけ使えます"))
            }
            if (paste) {
                val focus = remember { FocusRequester() }
                val submit = { if (link.isNotBlank() && !m.busy) submitLink(link) }
                LaunchedEffect(Unit) { focus.requestFocus() }
                // 欄そのものをカプセルにする（フォーカスで欄の全体がキーボードの上に出る）。
                BasicTextField(
                    link,
                    { link = it },
                    Modifier.padding(top = 16.dp)
                        .fillMaxWidth()
                        .heightIn(min = tap)
                        .background(pi.surface, CircleShape)
                        .padding(start = 16.dp, end = 2.dp)
                        .focusRequester(focus)
                        .semantics { contentDescription = "ペアリング用のリンク" },
                    textStyle = Type.body.copy(color = pi.text, fontFamily = Mono),
                    // 秘密のリンクを画面と IME の学習に残さない。
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions =
                        KeyboardOptions(
                            keyboardType = KeyboardType.Password,
                            imeAction = ImeAction.Go,
                        ),
                    keyboardActions = KeyboardActions(onGo = { submit() }),
                    singleLine = true,
                    cursorBrush = SolidColor(pi.accent),
                    decorationBox = { inner ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.weight(1f)) {
                                if (link.isEmpty())
                                    Text(
                                        "pimate://pair?…",
                                        color = pi.textSecondary,
                                        fontFamily = Mono,
                                    )
                                inner()
                            }
                            TextAction("接続", submit, enabled = link.isNotBlank() && !m.busy)
                        }
                    },
                )
            }
            SheetError(if (invalid) "ペアリング用のリンクを確認してください" else "")
            if (m.error.isNotEmpty())
                Text(
                    m.error,
                    Modifier.padding(top = 16.dp).semantics { liveRegion = LiveRegionMode.Polite },
                    pi.failedText,
                    style = Type.caption.copy(fontSize = 13.sp, lineHeight = 19.sp),
                )
            Spacer(Modifier.height(24.dp))
        }
        Column(
            Modifier.padding(start = gutter, top = 8.dp, end = gutter, bottom = 12.dp),
            Arrangement.spacedBy(4.dp),
        ) {
            PrimaryButton(
                if (m.busy) "ペアリング中…" else "QRコードを読み取る",
                {
                    scanner.launch(
                        ScanOptions()
                            .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                            .setBeepEnabled(false)
                            .setPrompt("PC に表示された QR コードを枠に合わせてください")
                    )
                },
                Modifier.fillMaxWidth(),
                enabled = !m.busy,
            )
            if (!paste)
                TextAction(
                    "リンクを貼り付ける",
                    { paste = true },
                    Modifier.fillMaxWidth(),
                    enabled = !m.busy,
                )
        }
    }
}

/** PC の Pi の見本: Pi のロゴ（4×4 のピクセル）と、上下の線にはさまれた入力欄。3 色はこの絵の中だけ。 */
@Composable
private fun PiTerminal(modifier: Modifier = Modifier) {
    val p = pi
    Column(
        modifier
            .fillMaxWidth()
            .background(p.surface, RoundedCornerShape(20.dp))
            .padding(start = 16.dp, top = 14.dp, end = 16.dp, bottom = 12.dp)
            .semantics(mergeDescendants = true) {},
        Arrangement.spacedBy(10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Canvas(Modifier.size(16.dp)) {
                val u = size.width / 4
                fun px(x: Int, y: Int, w: Int, h: Int, c: androidx.compose.ui.graphics.Color) =
                    drawRect(
                        c,
                        androidx.compose.ui.geometry.Offset(x * u, y * u),
                        androidx.compose.ui.geometry.Size(w * u, h * u),
                    )
                px(0, 0, 3, 1, p.piCoral)
                px(2, 1, 1, 1, p.piCoral)
                px(0, 1, 1, 3, p.piBlue)
                px(1, 2, 1, 1, p.piBlue)
                px(3, 2, 1, 2, p.piYellow)
            }
            Text("pi", Modifier.padding(start = 8.dp), p.textSecondary, style = Type.mono)
        }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(Modifier.fillMaxWidth().height(1.dp).background(p.piRule))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("/pimate setup", style = Type.mono.copy(fontSize = 14.sp, lineHeight = 20.sp))
                Box(Modifier.padding(start = 2.dp).size(8.dp, 17.dp).background(p.text))
            }
            Box(Modifier.fillMaxWidth().height(1.dp).background(p.piRule))
        }
        Text("PC の Pi でこう打つと、QR コードが表示されます", color = p.textSecondary, style = Type.caption)
    }
}

/** 等幅の番号タイルの手順。済み・未済はアプリから分からないので、チェックは付けない。 */
@Composable
private fun Step(number: Int, title: String, detail: AnnotatedString, open: (() -> Unit)? = null) {
    Row(
        Modifier.fillMaxWidth()
            .then(
                if (open != null) Modifier.clickable(role = Role.Button, onClick = open)
                else Modifier
            )
            .heightIn(min = 64.dp)
            .semantics(mergeDescendants = true) {},
        Arrangement.spacedBy(14.dp),
        Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(28.dp).background(pi.surface, RoundedCornerShape(8.dp)),
            Alignment.Center,
        ) {
            Text(
                "$number",
                color = pi.textCode,
                style = Type.mono.copy(fontSize = 13.sp, fontWeight = FontWeight.SemiBold),
            )
        }
        Column(Modifier.weight(1f)) {
            Text(title, style = Type.lead.copy(lineBreak = Type.phrase))
            Text(detail, color = pi.textSecondary, style = Type.caption)
        }
        if (open != null) Icon(Icons.Right, null, Modifier.size(16.dp), pi.textSecondary)
    }
}
