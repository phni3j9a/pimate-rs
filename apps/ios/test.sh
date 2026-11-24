#!/usr/bin/env bash
# Run on a *disposable*, shut-down simulator. No personal registration or TS Host is used.
set -euo pipefail
if [[ $# != 1 || ! $1 =~ ^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$ ]]; then
  echo 'usage: apps/ios/test.sh <disposable-simulator-UDID>' >&2; exit 64
fi
id=$1
umask 077
cd "$(dirname "$0")/../.."
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$PWD/target"
apps/ios/prepare.sh
lab=$(mktemp -d /tmp/pimate-ios.XXXXXX)
fixtures=(); links=(); booted=
cleanup() {
  for pid in ${fixtures[@]+"${fixtures[@]}"}; do kill -TERM "$pid" 2>/dev/null || :; wait "$pid" || :; done
  for file in ${links[@]+"${links[@]}"}; do rm -f "$file"; done
  if [[ -n $booted ]]; then xcrun simctl shutdown "$id"; fi
  rm -rf "$lab"
}
trap cleanup EXIT
xcrun simctl boot "$id"; booted=yes
xcrun simctl bootstatus "$id" -b
build="$PWD/apps/ios/build"
xcb=(xcodebuild -project apps/ios/PiMate.xcodeproj -scheme PiMate -destination "id=$id" -derivedDataPath "$build" -parallel-testing-enabled NO)
"${xcb[@]}" build-for-testing
xcrun simctl install "$id" "$build/Build/Products/Debug-iphonesimulator/PiMate.app"
link="$(xcrun simctl get_app_container "$id" dev.pimate.app.rs.debug data)/Library/Caches/pimate-test-link"
mkdir -p "$(dirname "$link")"
cargo build --locked -p pimate-host
start_fixture() {
  local n=$1
  if (( n <= 2 )); then file="$link-$n"; else file="$lab/link-$n"; fi
  links+=("$file")
  PIMATE_DEMO_NAME="デモ PC" scripts/android-demo.py >"$lab/fixture-$n" 2>&1 & fixtures+=("$!")
  for _ in {1..60}; do grep -q '^pimate://pair?' "$lab/fixture-$n" && break; kill -0 "${fixtures[$((n-1))]}"; sleep 1; done
  grep -m1 '^pimate://pair?' "$lab/fixture-$n" | tr -d '\r\n' >"$file"
  [[ -s $file ]]
}
start_fixture 1; start_fixture 2
# HTTP-only image fixture, bound to loopback and serving no checkout or pairing data.
portfile="$link-http-port"; links+=("$portfile")
python3 - "$portfile" <<'PYHTTP' &
import base64, http.server, sys
from pathlib import Path
class Image(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200); self.end_headers()
        self.wfile.write(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO6pXioAAAAASUVORK5CYII='))
    def log_message(self, *args): pass
with http.server.HTTPServer(('127.0.0.1', 0), Image) as server:
    Path(sys.argv[1]).write_text(str(server.server_port)); server.serve_forever()
PYHTTP
fixtures+=("$!")
result="$build/Tests-$(date +%s).xcresult"
TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 "${xcb[@]}" -only-testing:PiMateTests -resultBundlePath "$result" test-without-building
[[ ! -e $link-1 && ! -e $link-2 ]]
for variant in light dark large; do
  mode=light; size=large
  [[ $variant != dark ]] || mode=dark
  [[ $variant != large ]] || size=extra-extra-extra-large
  xcrun simctl ui "$id" appearance "$mode"
  xcrun simctl ui "$id" content_size "$size"
  start_fixture "$((${#fixtures[@]} + 1))"
  xcrun simctl pbcopy "$id" <"$file"; rm -f "$file"
  result="$build/$variant-$(date +%s).xcresult"
  TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 TEST_RUNNER_PIMATE_VARIANT="$variant" "${xcb[@]}" -only-testing:PiMateUITests -resultBundlePath "$result" test-without-building
  mkdir -p "$build/attachments/$variant"
  xcrun xcresulttool export attachments --path "$result" --output-path "$build/attachments/$variant"
done
