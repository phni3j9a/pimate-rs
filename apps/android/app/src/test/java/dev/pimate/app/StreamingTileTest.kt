package dev.pimate.app

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.pimate_ffi.ImageKind
import uniffi.pimate_ffi.SharedTextInterface

class StreamingTileTest {
    @Test
    fun reparsingSameVisibleFileDoesNotCancelOrRestartItsRequest() = runBlocking {
        val started = CompletableDeferred<Unit>()
        val release = CompletableDeferred<Unit>()
        val loaded = CompletableDeferred<Unit>()
        val requests = mutableListOf<Pic>()
        var reads = 0
        fun handle(path: String) =
            Pic.Shared(
                ImageKind.FILE,
                object : SharedTextInterface {
                    override fun value() = path.also { reads++ }
                },
            )
        val sources =
            flow<Pic> {
                    emit(handle("a.png"))
                    started.await()
                    // 同じタイルを再解析したときも SharedText のハンドルは新しくなる。
                    repeat(3) { emit(handle("a.png")) }
                    release.complete(Unit)
                    loaded.await()
                    emit(handle("b.png")) // 後から届いた参照定義で参照先が変わった。
                }
                .tileSources()
        assertEquals(0, reads)
        withTimeout(5000) {
            sources.collectLatest { source ->
                requests += requireNotNull(source)
                if (source == Pic.File("a.png")) {
                    started.complete(Unit)
                    release.await()
                    loaded.complete(Unit)
                }
            }
        }
        assertEquals(5, reads)
        assertEquals(listOf(Pic.File("a.png"), Pic.File("b.png")), requests)
    }

    @Test
    fun repeatedDataUrisUseTheirContentsAndUnreadableHandlesRecover() = runBlocking {
        fun ref(value: String?) =
            Pic.Shared(
                ImageKind.DATA,
                object : SharedTextInterface {
                    override fun value() = requireNotNull(value)
                },
            )
        val a = "data:image/png;base64,YQ=="
        val b = "data:image/png;base64,Yg=="
        val slots = flowOf(ref(a), ref(a), ref(null), ref(b), ref(b)).tileSources().toList()
        assertEquals(
            listOf(Pic.Inline("", "image/png", "YQ=="), null, Pic.Inline("", "image/png", "Yg==")),
            slots,
        )
    }
}
