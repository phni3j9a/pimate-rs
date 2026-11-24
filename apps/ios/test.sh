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
# Only generated swatches enter this disposable photo library.
python3 - "$lab" <<'PHOTOS'
import base64, runpy, sys
from pathlib import Path
picture = runpy.run_path('scripts/android-demo.py')['picture']
for n in range(4):
    (Path(sys.argv[1]) / f'swatch-{n}.png').write_bytes(base64.b64decode(picture(n, True)['data']))
PHOTOS
xcrun simctl addmedia "$id" "$lab"/swatch-*.png
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
result="$build/Tests-$(date +%s).xcresult"
TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 "${xcb[@]}" -only-testing:PiMateTests -resultBundlePath "$result" test-without-building
[[ ! -e $link-1 && ! -e $link-2 ]]
failed=0
for variant in light dark large; do
  mode=light; size=large
  [[ $variant != dark ]] || mode=dark
  [[ $variant != large ]] || size=extra-extra-extra-large
  xcrun simctl ui "$id" appearance "$mode"
  xcrun simctl ui "$id" content_size "$size"
  start_fixture "$((${#fixtures[@]} + 1))"
  xcrun simctl pbcopy "$id" <"$file"; rm -f "$file"
  result="$build/$variant-$(date +%s).xcresult"
  if TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 TEST_RUNNER_PIMATE_VARIANT="$variant" "${xcb[@]}" -only-testing:PiMateUITests -resultBundlePath "$result" test-without-building; then
    # The last copy is a table from the demo, not just its visible cells. Never log the clipboard.
    xcrun simctl pbpaste "$id" >"$lab/copied"
    python3 - "$lab/copied" <<'TABLE' || failed=1
import runpy, sys
from pathlib import Path
body = runpy.run_path('scripts/android-demo.py')['BODY']
tables = [p for p in body.split('\n\n') if p.startswith('|')]
if Path(sys.argv[1]).read_text().rstrip('\n') not in tables:
    sys.exit('Copied text did not match a complete fixture Markdown table')
TABLE
  else
    failed=1
  fi
  mkdir -p "$build/attachments/$variant"
  xcrun xcresulttool export attachments --path "$result" --output-path "$build/attachments/$variant"
done
exit "$failed"
