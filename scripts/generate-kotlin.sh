#!/usr/bin/env bash
set -euo pipefail
cargo run --locked -p pimate-ffi --features bindgen --bin pimate-bindgen -- generate --no-format -l kotlin -o "$1" "$2"
# UniFFI 0.32 only catches Exception in synchronous callbacks. Never let a JVM Error
# cross JNA (or serialize a key store's stack trace). Fail closed on template changes.
python3 - "$1/uniffi/pimate_ffi/pimate_ffi.kt" <<'PY'
from pathlib import Path
import re
import sys
p = Path(sys.argv[1])
s = p.read_text()
for old, new in [('catch(e: kotlin.Exception)', 'catch(e: Throwable)'),
                 ('val err = try { e.stackTraceToString() } catch(_: Throwable) { "" }', 'val err = "callback failed"')]:
    if s.count(old) != 2:
        sys.exit('UniFFI callback template changed; review its exception boundary')
    s = s.replace(old, new)
# JNA 5.17 shares the detach flag between nested callbacks on a native thread.
# Configure every outer entry point (future wakes and foreign-interface vtables)
# before JNA creates its trampoline. Native worker exit, not callback return, detaches.
s, count = re.subn(r'(?m)^([ ]+)override fun callback\(',
    r'\1init { com.sun.jna.Native.setCallbackThreadInitializer(this, com.sun.jna.CallbackThreadInitializer(true, false)) }\n\1override fun callback(', s)
if count != 11:
    sys.exit('UniFFI JNA callbacks changed; review their thread initializers')
p.write_text(s)
PY
