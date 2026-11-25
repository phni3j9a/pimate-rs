#!/bin/sh
# LaunchServices supplies the foreground application identity used by the macOS IME.
set -eu
cd "$(dirname "$0")/../.."
cargo build --locked -p pimate-desktop
out="${CARGO_TARGET_DIR:-target}/debug"
app="$out/PiMate RS.app"
mkdir -p "$app/Contents/MacOS"
cp apps/desktop/Info.plist "$app/Contents/Info.plist"
# A rebuild replaces the binary; point the development bundle at that exact inode.
ln -f "$out/pimate-desktop" "$app/Contents/MacOS/pimate-desktop"
exec open "$app" --args "$@"
