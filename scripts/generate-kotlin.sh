#!/usr/bin/env bash
set -euo pipefail
cargo run --locked -p pimate-ffi --features bindgen --bin pimate-bindgen -- generate --no-format -l kotlin -o "$1" "$2"
# UniFFI 0.32 only catches Exception in synchronous callbacks. Never let a JVM Error
# cross JNA (or serialize a key store's stack trace). Fail closed on template changes.
python3 - "$1/uniffi/pimate_ffi/pimate_ffi.kt" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
s = p.read_text()
for old, new in [('catch(e: kotlin.Exception)', 'catch(e: Throwable)'),
                 ('val err = try { e.stackTraceToString() } catch(_: Throwable) { "" }', 'val err = "callback failed"')]:
    if s.count(old) != 2:
        sys.exit('UniFFI callback template changed; review its exception boundary')
    s = s.replace(old, new)
p.write_text(s)
PY
