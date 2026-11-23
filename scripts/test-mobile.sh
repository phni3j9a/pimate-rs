#!/usr/bin/env bash
# Compile a native smoke client, then have the isolated Rust Host fixture drive it.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
out="$CARGO_TARGET_DIR/mobile/smoke"
debug="$CARGO_TARGET_DIR/debug"
client=crates/pimate-ffi/tests/native
case ${1:-} in swift) lib="$debug/libpimate_ffi.dylib";; kotlin) lib="$debug/libpimate_ffi.so";; *) echo 'usage: test-mobile.sh kotlin|swift' >&2; exit 2;; esac
cargo build --locked -p pimate-ffi --lib
cargo run --locked -p pimate-ffi --features bindgen --bin pimate-bindgen -- generate --no-format -l "$1" -o "$out" "$lib"
if [[ $1 == swift ]]; then
  swiftc -Xcc "-fmodule-map-file=$out/pimate_ffiFFI.modulemap" -I "$out" -L "$debug" -lpimate_ffi \
    "$out/pimate_ffi.swift" "$client/Smoke.swift" -o "$out/client"
  export DYLD_LIBRARY_PATH="$debug"
else
  : "${KOTLIN_CP:?set to JNA, Kotlin stdlib and kotlinx-coroutines-core-jvm jars (colon separated)}"
  kotlinc -cp "$KOTLIN_CP" "$out/uniffi/pimate_ffi/pimate_ffi.kt" "$client/Smoke.kt" -d "$out/client.jar"
  export CLASSPATH="$out/client.jar:$KOTLIN_CP" LD_LIBRARY_PATH="$debug${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  printf '#!/bin/sh\nexec java -Djna.library.path="$LD_LIBRARY_PATH" SmokeKt\n' > "$out/client"
  chmod +x "$out/client"
fi
TMPDIR=/tmp PIMATE_FFI_CLIENT="$out/client" cargo test --locked -p pimate-host --test host foreign_client_receives_diffs -- --ignored
