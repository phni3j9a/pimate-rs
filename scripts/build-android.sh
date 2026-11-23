#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
out="$CARGO_TARGET_DIR/mobile/android"
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o "$out/jniLibs" build --release --locked -p pimate-ffi --lib
cargo run --locked -p pimate-ffi --features bindgen --bin pimate-bindgen -- generate \
  --no-format -l kotlin -o "$out/kotlin" "$out/jniLibs/arm64-v8a/libpimate_ffi.so"
