package dev.pimate.app

import android.content.ContentResolver
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import android.net.Uri
import android.util.Base64
import java.io.ByteArrayOutputStream
import uniffi.pimate_ffi.Image

internal fun imageBudget(existing: List<Image>, count: Int, limit: Int): Int {
    require(count > 0 && existing.size + count <= minOf(limit, 4)) { "添付できる枚数を超えています" }
    val used =
        existing.sumOf { it.data.length / 4 * 3 - it.data.takeLast(2).count { c -> c == '=' } }
    return minOf(512 * 1024, (600 * 1024 - used) / count).also { require(it > 0) }
}

// URI grants come from the system photo picker. Do not retain paths, EXIF or original bytes.
internal fun encodePhoto(resolver: ContentResolver, uri: Uri, budget: Int): Image {
    require(uri.scheme == ContentResolver.SCHEME_CONTENT)
    return encodePhoto(budget) { requireNotNull(resolver.openInputStream(uri)) }
}

internal fun encodePhoto(budget: Int, open: () -> java.io.InputStream): Image {
    val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    open().use { BitmapFactory.decodeStream(it, null, options) }
    require(options.outWidth > 0 && options.outHeight > 0) { "画像を読み込めません" }
    val orientation =
        open().use { ExifInterface(it).getAttributeInt(ExifInterface.TAG_ORIENTATION, 1) }
    options.inJustDecodeBounds = false
    options.inSampleSize = 1
    while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > 3200) options
        .inSampleSize *= 2
    val bitmap = requireNotNull(open().use { BitmapFactory.decodeStream(it, null, options) })
    try {
        for ((edge, quality) in listOf(1600 to 85, 1200 to 70, 800 to 55, 512 to 40, 320 to 30)) {
            val scale = minOf(1f, edge.toFloat() / maxOf(bitmap.width, bitmap.height))
            val matrix =
                Matrix().apply {
                    setScale(if (orientation in listOf(2, 4, 5, 7)) -scale else scale, scale)
                    postRotate(
                        when (orientation) {
                            3,
                            4 -> 180f
                            5,
                            8 -> 270f
                            6,
                            7 -> 90f
                            else -> 0f
                        }
                    )
                }
            val small = Bitmap.createBitmap(bitmap, 0, 0, bitmap.width, bitmap.height, matrix, true)
            try {
                val bytes = ByteArrayOutputStream()
                if (
                    small.compress(Bitmap.CompressFormat.JPEG, quality, bytes) &&
                        bytes.size() in 1..budget
                )
                    return Image(
                        "image/jpeg",
                        Base64.encodeToString(bytes.toByteArray(), Base64.NO_WRAP),
                    )
            } finally {
                if (small !== bitmap) small.recycle()
            }
        }
        error("画像を小さくしても添付の上限を超えています")
    } finally {
        bitmap.recycle()
    }
}
