#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target}
export IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET:-16.0}
out="$CARGO_TARGET_DIR/mobile/ios"
for arch in aarch64-apple-ios aarch64-apple-ios-sim; do
  cargo build --locked --release --target "$arch" -p pimate-ffi --lib
done
cargo build --locked -p pimate-ffi --lib
cargo run --locked -p pimate-ffi --features bindgen --bin pimate-bindgen -- generate \
  --no-format -l swift -o "$out" "$CARGO_TARGET_DIR/debug/libpimate_ffi.dylib"
mkdir -p "$out/include"
cp "$out/pimate_ffiFFI.h" "$out/include/"
cp "$out/pimate_ffiFFI.modulemap" "$out/include/module.modulemap"
rm -rf "$out/PiMate.xcframework"
xcodebuild -create-xcframework \
  -library "$CARGO_TARGET_DIR/aarch64-apple-ios/release/libpimate_ffi.a" -headers "$out/include" \
  -library "$CARGO_TARGET_DIR/aarch64-apple-ios-sim/release/libpimate_ffi.a" -headers "$out/include" \
  -output "$out/PiMate.xcframework"
