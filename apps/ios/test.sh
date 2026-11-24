#!/usr/bin/env bash
# Run on a *disposable*, shut-down simulator. No personal registration or TS Host is used.
set -euo pipefail
umask 077
cd "$(dirname "$0")/../.."
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$PWD/target"
apps/ios/prepare.sh
id=${1:-$(xcrun simctl list devices available -j | python3 -c 'import json,sys; print(next(d["udid"] for k,ds in json.load(sys.stdin)["devices"].items() if ".iOS-" in k for d in ds if d["name"].startswith("iPhone") and d["state"]=="Shutdown"))')}
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
for n in 1 2; do
  file="$link-$n"; links+=("$file")
  scripts/android-demo.py >"$lab/fixture-$n" 2>&1 & fixtures+=("$!")
  for _ in {1..60}; do grep -q '^pimate://pair?' "$lab/fixture-$n" && break; kill -0 "${fixtures[$((n-1))]}"; sleep 1; done
  grep -m1 '^pimate://pair?' "$lab/fixture-$n" | tr -d '\r\n' >"$file"
  [[ -s $file ]]
done
result="$build/Tests-$(date +%s).xcresult"
TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 "${xcb[@]}" -resultBundlePath "$result" test-without-building
[[ ! -e $link-1 && ! -e $link-2 ]] # A skipped fixture test must never make CI green.
xcrun xcresulttool export attachments --path "$result" --output-path "$build/attachments"
