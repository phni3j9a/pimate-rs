package dev.pimate.app

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.*
import androidx.compose.ui.text.font.*
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.*
import kotlinx.coroutines.*
import uniffi.pimate_ffi.*

@Composable fun Markdown(source: String) {
    val blocks by produceState(emptyList<Block>(), source) { value = withContext(Dispatchers.Default) { parse(source,false).use { it.blocks() } } }
    SelectionContainer { Column { blocks.forEach { BlockView(it) } } }
}
@Composable private fun Spans(spans: List<Inline>, bold: Boolean = false) {
    Text(remember(spans, bold) { buildAnnotatedString { spans.forEach { span -> when(span) {
        is Inline.Image -> append(span.v1.alt)
        is Inline.Text -> with(span.style) {
            val url = href?.value(); if (url != null) pushLink(LinkAnnotation.Url(url))
            withStyle(SpanStyle(fontWeight = if (bold || span.style.bold) FontWeight.Bold else null,
                fontStyle = if (italic) FontStyle.Italic else null, fontFamily = if (code) FontFamily.Monospace else null,
                textDecoration = if (strike) TextDecoration.LineThrough else if (url != null) TextDecoration.Underline else null)) { append(span.text) }; if (url != null) pop()
        }
    } } } })
}
@Composable private fun BlockView(block: Block) {
    when(val c = block.content) {
        is Content.Paragraph -> Spans(c.v1)
        is Content.Heading -> Spans(c.spans,true)
        is Content.Code -> Text(c.text.replace("\t","  "), Modifier.horizontalScroll(rememberScrollState()), fontFamily = FontFamily.Monospace)
        is Content.Quote -> Column(Modifier.padding(start = gutter)) { c.v1.forEach { BlockView(it) } }
        is Content.BlockList -> c.items.forEachIndexed { i,it -> Row { Text(c.start?.let { "${it.toLong()+i}. " } ?: "• "); BlockView(it) } }
        is Content.Item -> Column { c.children.forEach { BlockView(it) } }
        is Content.Table -> Column(Modifier.horizontalScroll(rememberScrollState())) { (listOf(c.v1.header)+c.v1.rows).forEachIndexed { index,row -> Row { c.v1.header.indices.forEach { col -> Box(Modifier.width(120.dp).padding(4.dp)) { Spans(row.getOrElse(col) { emptyList() }, index == 0) } } } } }
        Content.Rule -> HorizontalDivider()
    }
}
