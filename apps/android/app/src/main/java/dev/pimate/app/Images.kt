package dev.pimate.app

import android.graphics.BitmapFactory
import android.util.Base64
import android.util.LruCache
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONObject
import uniffi.pimate_ffi.RcMethod

// 画像はメモリにだけ置く（ディスクに保存しない。docs/SPEC.md「会話の画像」）。
private const val CACHE_BYTES = 24 * 1024 * 1024
private const val WEB_BYTES = 5 * 1024 * 1024
private const val TILE_EDGE = 800
private const val FULL_EDGE = 4096

// タイルの画像（縮小して復号したもの）。スクロールで戻ったときに復号・取得し直さない。全画面の画像は入れない。
private val tiles =
    object : LruCache<String, ImageBitmap>(CACHE_BYTES) {
        override fun sizeOf(key: String, value: ImageBitmap) =
            value.asAndroidBitmap().byteCount + key.length * 2
    }

// shortcut: EXIF の向きは見ない（送った写真は添付の時点で直してある）。PC のカメラ写真を回して出すなら、復号の前に向きを読む。
private fun decode(bytes: ByteArray, edge: Int): ImageBitmap? {
    val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
    if (options.outWidth <= 0 || options.outHeight <= 0) return null
    options.inJustDecodeBounds = false
    options.inSampleSize = 1
    while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > edge) options
        .inSampleSize *= 2
    return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.asImageBitmap()
}

private fun fetch(url: String): ByteArray {
    val c = URL(url).openConnection() as HttpURLConnection
    try {
        c.connectTimeout = 10_000
        c.readTimeout = 15_000
        c.useCaches = false
        check(c.responseCode in 200..299)
        return c.inputStream.use { input ->
            val out = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(16 * 1024)
            while (true) {
                val n = input.read(buffer)
                if (n < 0) break
                out.write(buffer, 0, n)
                check(out.size() <= WEB_BYTES)
            }
            out.toByteArray()
        }
    } finally {
        c.disconnect()
    }
}

/**
 * 画像の取得。ChatScreen が、会話を開いた時点の EditorKey に結び付けて作る（対象が変わったあとの結果は request が捨てる）。 scope
 * はキャッシュの名前空間（Host・Instance・セッション）。
 */
internal class Pics(
    val scope: String,
    /** entry_image が使える（省かれたプレビューを取りに行ける）。 */
    val entries: Boolean,
    /** file_image が使える（Markdown の PC のパスを画像として出す）。 */
    val files: Boolean,
    private val request: suspend (RcMethod, JSONObject) -> String,
    /** 全画面で開く。 */
    val open: (List<Pic>, Int) -> Unit = { _, _ -> },
) {
    // Keep compressed bytes rather than retaining a second full-size bitmap after dismissal.
    private val originals =
        object : LruCache<Pic, ByteArray>(CACHE_BYTES) {
            override fun sizeOf(key: Pic, value: ByteArray) = value.size
        }
    private val originalLoad = Mutex()

    fun cached(pic: Pic): ImageBitmap? = if (pic.id.isEmpty()) null else tiles["$scope|${pic.id}"]

    /** 読めなければ null。tile: 会話に置く縮小版（RC には thumbnail を頼む）。全画面は別に取りに行く。 */
    suspend fun load(pic: Pic, tile: Boolean): ImageBitmap? {
        if (tile)
            cached(pic)?.let {
                return it
            }
        val edge = if (tile) TILE_EDGE else FULL_EDGE
        val image =
            try {
                when (pic) {
                    is Pic.Shared -> load(pic.resolve(), tile)
                    is Pic.Inline -> inline(pic.data, edge)
                    is Pic.Web -> withContext(Dispatchers.IO) { decode(fetch(pic.url), edge) }
                    is Pic.Entry ->
                        remote(
                            pic,
                            RcMethod.ENTRY_IMAGE,
                            JSONObject()
                                .put("entryId", pic.entryId)
                                .put("index", pic.index)
                                .put("thumbnail", tile),
                            edge,
                        )
                    is Pic.File ->
                        remote(
                            pic,
                            RcMethod.FILE_IMAGE,
                            JSONObject().put("path", pic.path).put("thumbnail", tile),
                            edge,
                        )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                null
            }
        if (tile && image != null && pic.id.isNotEmpty()) tiles.put("$scope|${pic.id}", image)
        return image
    }

    private suspend fun inline(data: String, edge: Int) =
        withContext(Dispatchers.Default) { decode(Base64.decode(data, Base64.DEFAULT), edge) }

    private suspend fun remote(
        pic: Pic,
        method: RcMethod,
        params: JSONObject,
        edge: Int,
    ): ImageBitmap? {
        suspend fun fetchData() =
            JSONObject(request(method, params)).getJSONObject("image").getString("data")
        if (edge != FULL_EDGE || pic is Pic.File && pic.answer == null)
            return inline(fetchData(), edge)
        return originalLoad.withLock {
            val cached = originals[pic]
            val bytes =
                cached
                    ?: fetchData().let { data ->
                        withContext(Dispatchers.Default) { Base64.decode(data, Base64.DEFAULT) }
                    }
            withContext(Dispatchers.Default) { decode(bytes, edge) }
                ?.also { if (cached == null) originals.put(pic, bytes) }
        }
    }

    companion object {
        /** 会話の外（添付のプレビューなど）。手元にある画像と Web の画像だけ読める。 */
        val Local = Pics("", false, false, { _, _ -> error("no target") })
    }
}

internal val LocalPics = staticCompositionLocalOf { Pics.Local }

private object Failed

/** 画像のタイル。読み込み中は印、読めなければ文字で示す。タップで全画面を開く。 */
@Composable
internal fun PicTile(
    pic: Pic,
    label: String,
    compact: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    /** 4 枚目に重ねる「+N」。 */
    more: Int = 0,
) {
    val pics = LocalPics.current
    // 一つの回答全体が Column に載る。画面外の参照画像は base64 も Bitmap も展開しない。
    var visible by remember(pics) { mutableStateOf(false) }
    val needed = pic !is Pic.Shared || visible
    var measured by remember(pics) { mutableStateOf(IntSize.Zero) }
    val current by rememberUpdatedState(pic)
    val image by
        produceState<Any?>(pics.cached(pic), pics, needed, more) {
            if (!needed || more != 0) {
                value = null
                return@produceState
            }
            snapshotFlow { current }
                .tileSources()
                .collectLatest { src ->
                    if (src == null) {
                        value = Failed
                        return@collectLatest
                    }
                    value = pics.cached(src)
                    if (value == null) value = pics.load(src, true) ?: Failed
                    // 画面外で Bitmap を解放しても位置が飛ばないよう、幅と高さだけ残す。
                    (value as? ImageBitmap)?.let { measured = IntSize(it.width, it.height) }
                }
        }
    val shown = image as? ImageBitmap
    // 1 枚だけのときは、画像の縦横比のまま 220×160dp に収める。
    val size =
        if (compact || measured.width == 0)
            Modifier.size(if (compact) 64.dp else 180.dp, if (compact) 64.dp else 140.dp)
        else {
            val scale = minOf(220f / measured.width, 160f / measured.height)
            Modifier.size((measured.width * scale).dp, (measured.height * scale).dp)
        }
    Box(
        modifier
            .then(size)
            .then(
                if (pic is Pic.Shared)
                    Modifier.onGloballyPositioned { visible = !it.boundsInWindow().isEmpty }
                else Modifier
            )
            .clip(RoundedCornerShape(if (compact) 12.dp else 16.dp))
            .background(pi.surfaceStrong)
            .clickable(onClickLabel = "全画面で開く", role = Role.Image, onClick = onClick)
            .semantics {
                contentDescription =
                    when {
                        more > 0 -> "残り $more 枚の画像"
                        image === Failed -> "$label、読み込めません"
                        else -> label
                    }
            },
        Alignment.Center,
    ) {
        when {
            more > 0 -> Text("+$more", style = Type.sheet)
            shown != null ->
                Image(
                    shown,
                    null,
                    Modifier.fillMaxSize(),
                    contentScale = if (compact) ContentScale.Crop else ContentScale.Fit,
                )
            image === Failed ->
                Text(
                    if (compact) "開く" else "画像を読み込めません",
                    Modifier.padding(6.dp),
                    pi.textSecondary,
                    style = Type.meta.copy(textAlign = TextAlign.Center),
                )
            else -> Spinner()
        }
    }
}

/** 発言・ツール結果の画像。最大 2×2 に並べ、5 枚以上は 4 枚目を「+N」にして全画面で見る。 */
@Composable
internal fun PicGrid(all: ImageSlots, modifier: Modifier = Modifier) {
    val pics = LocalPics.current
    val shown = remember(all, pics.entries) { all.gallery(pics.entries) }
    Column(modifier, Arrangement.spacedBy(8.dp)) {
        shown.take(4).chunked(2).forEachIndexed { row, pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                pair.forEachIndexed { col, pic ->
                    val i = row * 2 + col
                    PicTile(
                        pic,
                        "画像 ${i + 1} / ${shown.size}",
                        compact = shown.size > 1,
                        { pics.open(shown, i) },
                        more = if (i == 3 && shown.size > 4) shown.size - 3 else 0,
                    )
                }
            }
        }
        // 取りに行けない RC では、出せなかった画像があることを文字で残す。
        if (shown.size < all.size)
            Text(
                if (shown.isEmpty()) "画像 ${all.size} 枚（この Pi からは表示できません）"
                else "画像 ${all.size} 枚（残りは表示できません）",
                color = pi.textSecondary,
                style = Type.meta,
            )
    }
}

/** 全画面の画像（写真の閲覧と同じく、テーマによらず黒）。横に送って切り替え、つまむ・2 回タップで拡大する。 */
@Composable
internal fun ImageViewer(all: List<Pic>, start: Int, close: () -> Unit) {
    FullScreen(close, dark = true) {
        val pager = rememberPagerState(start.coerceIn(0, all.lastIndex)) { all.size }
        var zoomed by remember { mutableStateOf(false) }
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            HorizontalPager(
                pager,
                Modifier.fillMaxSize(),
                userScrollEnabled = !zoomed,
                beyondViewportPageCount = 0,
            ) { page ->
                ViewerPage(
                    all[page],
                    if (all.size > 1) "画像 ${page + 1} / ${all.size}" else "画像",
                    active = page == pager.currentPage,
                    close = close,
                ) {
                    if (page == pager.currentPage) zoomed = it
                }
            }
            Row(
                Modifier.fillMaxWidth().safeDrawingPadding().padding(horizontal = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconAction(Icons.Close, "閉じる", close, tint = Color.White)
                if (all.size > 1)
                    Text(
                        "${pager.currentPage + 1} / ${all.size}",
                        Modifier.weight(1f).padding(end = tap).semantics {
                            liveRegion = LiveRegionMode.Polite
                        },
                        Color.White,
                        style = Type.lead.copy(textAlign = TextAlign.Center),
                    )
            }
        }
    }
}

@Composable
private fun ViewerPage(
    pic: Pic,
    label: String,
    active: Boolean,
    close: () -> Unit,
    onZoom: (Boolean) -> Unit,
) {
    val pics = LocalPics.current
    val image by produceState<Any?>(null, pic, pics) { value = pics.load(pic, false) ?: Failed }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    var box by remember { mutableStateOf(IntSize.Zero) }
    // ほかのページへ送ったら、等倍に戻す。
    LaunchedEffect(active) {
        if (!active) {
            scale = 1f
            offset = Offset.Zero
        }
    }
    val shown = image as? ImageBitmap
    fun settle(next: Float, at: Offset) {
        scale = next.coerceIn(1f, 4f)
        // 拡大した画像の端が、画面の内側に入らないようにする。
        val fit =
            if (shown == null || box.width == 0) 1f
            else minOf(box.width.toFloat() / shown.width, box.height.toFloat() / shown.height)
        val maxX = (((shown?.width ?: 0) * fit * scale - box.width) / 2).coerceAtLeast(0f)
        val maxY = (((shown?.height ?: 0) * fit * scale - box.height) / 2).coerceAtLeast(0f)
        offset = Offset(at.x.coerceIn(-maxX, maxX), at.y.coerceIn(-maxY, maxY))
        onZoom(scale > 1.01f)
    }
    Box(
        Modifier.fillMaxSize()
            .onSizeChanged { box = it }
            .pointerInput(shown) {
                detectTapGestures(
                    onDoubleTap = { tap ->
                        if (scale > 1.01f) settle(1f, Offset.Zero)
                        else {
                            val center = Offset(size.width / 2f, size.height / 2f)
                            settle(2.5f, (center - tap) * 1.5f)
                        }
                    }
                )
            }
            .pointerInput(shown) {
                awaitEachGesture {
                    val start = awaitFirstDown(requireUnconsumed = false).position
                    var dismissible = scale == 1f
                    var drag = Offset.Zero
                    do {
                        val event = awaitPointerEvent()
                        // 等倍で指 1 本の横送りは、ページの切り替えに渡す。
                        if (event.changes.size > 1 || scale > 1.01f) {
                            dismissible = false
                            val zoom = event.calculateZoom()
                            val next = (scale * zoom).coerceIn(1f, 4f)
                            settle(next, offset * (next / scale) + event.calculatePan())
                            event.changes.forEach { if (it.positionChanged()) it.consume() }
                        } else if (dismissible) {
                            val change = event.changes.single()
                            if (change.isConsumed && change.position != change.previousPosition)
                                dismissible = false
                            drag = change.position - start
                            if (
                                drag.y > viewConfiguration.touchSlop &&
                                    drag.y > kotlin.math.abs(drag.x)
                            )
                                change.consume()
                        }
                    } while (event.changes.any { it.pressed })
                    if (
                        dismissible &&
                            drag.y > size.height * .15f &&
                            drag.y > kotlin.math.abs(drag.x)
                    )
                        close()
                    if (scale <= 1.01f) settle(1f, Offset.Zero)
                }
            },
        Alignment.Center,
    ) {
        when {
            shown != null ->
                Image(
                    shown,
                    label,
                    Modifier.fillMaxSize().graphicsLayer {
                        scaleX = scale
                        scaleY = scale
                        translationX = offset.x
                        translationY = offset.y
                    },
                    contentScale = ContentScale.Fit,
                )
            image === Failed -> Text("画像を読み込めません", color = Color.White.copy(alpha = .72f))
            else -> CircularProgressIndicator(Modifier.size(28.dp), Color.White, 2.dp)
        }
    }
}
