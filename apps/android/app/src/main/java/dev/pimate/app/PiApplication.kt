package dev.pimate.app

import android.app.Application
import uniffi.pimate_ffi.initLogging

class PiApplication : Application() {
    // Keep compare-and-invalidate atomic across activity / ViewModel recreation.
    val store by lazy { EncryptedStore(this) }

    override fun onCreate() {
        super.onCreate()
        initLogging()
    }
}
