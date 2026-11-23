package dev.pimate.app

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.*
import org.junit.Test

class PhotoContract {
    @Test
    fun encodeScalesAndStripsOriginal() {
        val dir = InstrumentationRegistry.getInstrumentation().targetContext.cacheDir
        val file = File.createTempFile("photo", ".png", dir)
        try {
            val bitmap = Bitmap.createBitmap(2000, 100, Bitmap.Config.ARGB_8888)
            try {
                file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            } finally {
                bitmap.recycle()
            }
            val image = file.inputStream().use { encodePhoto(it.fd, 512 * 1024) }
            val bytes = Base64.decode(image.data, Base64.DEFAULT)
            assertEquals("image/jpeg", image.mimeType)
            val decoded = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
            try {
                assertEquals(1600, decoded.width)
                assertEquals(80, decoded.height)
            } finally {
                decoded.recycle()
            }
        } finally {
            file.delete()
        }
    }
}
