package dev.pimate.app

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.*
import com.journeyapps.barcodescanner.*

@Composable
fun PairScreen(m: SessionModel) {
    var link by remember { mutableStateOf("") }
    val scanner =
        rememberLauncherForActivityResult(ScanContract()) { r -> r.contents?.let { m.pair(it) } }
    Text("PC とペアリング", style = MaterialTheme.typography.titleMedium)
    OutlinedTextField(
        link,
        { link = it },
        Modifier.fillMaxWidth(),
        label = { Text("ペアリングリンク") },
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        singleLine = true,
    )
    Button({ m.pair(link) }, enabled = link.isNotBlank() && !m.busy) { Text("接続") }
    TextButton(
        {
            scanner.launch(
                ScanOptions()
                    .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                    .setBeepEnabled(false)
                    .setPrompt("PC の QR コードを読み取る")
            )
        },
        enabled = !m.busy,
    ) {
        Text("QR コードを読み取る")
    }
}
