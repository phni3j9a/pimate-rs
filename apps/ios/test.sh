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
export PIMATE_RUN_DIR="$lab/run" PIMATE_CONFIG_DIR="$lab/config" PI_CODING_AGENT_DIR="$lab/agent" XDG_CONFIG_HOME="$lab/xdg"
fixtures=(); links=(); booted=
stop_fixtures() {
  for pid in ${fixtures[@]+"${fixtures[@]}"}; do kill -TERM "$pid" 2>/dev/null || :; wait "$pid" || :; done
  fixtures=()
}
cleanup() {
  stop_fixtures
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
  local n=$1 fixture_pid
  # Completed journeys no longer need their live Hosts and streaming mock agents.
  if (( n > 2 )); then stop_fixtures; fi
  if (( n <= 2 )); then file="$link-$n"; else file="$lab/link-$n"; fi
  links+=("$file")
  PIMATE_DEMO_NAME="デモ PC" scripts/android-demo.py >"$lab/fixture-$n" 2>&1 &
  fixture_pid=$!; fixtures+=("$fixture_pid")
  for _ in {1..60}; do grep -q '^pimate://pair?' "$lab/fixture-$n" && break; kill -0 "$fixture_pid"; sleep 1; done
  grep -m1 '^pimate://pair?' "$lab/fixture-$n" | tr -d '\r\n' >"$file"
  [[ -s $file ]]
  # Trust only the port announced by this process; never derive the expected address from a link.
  fixture_port=$(awk '$1 == "adb" && $4 == "reverse" && $5 == $6 && $5 ~ /^tcp:[0-9]+$/ {print substr($5,5)}' "$lab/fixture-$n")
  [[ $fixture_port =~ ^[0-9]+$ ]] && (( fixture_port > 0 && fixture_port <= 65535 && fixture_port != 47800 )) || return 1
  # Exercise the same HTTP listener (including the management proxy) without consuming the link.
  curl --noproxy '*' --silent --show-error --fail --max-time 3 --retry 5 --retry-all-errors --retry-delay 1 \
    "http://127.0.0.1:$fixture_port/health" > /dev/null
  export TEST_RUNNER_PIMATE_FIXTURE_PORT=$fixture_port
}
# Reuse the loopback management fixture for Host 1 only. Never contact an actual provider.
ws_version=$(node -p 'require("./packages/pimate/package-lock.json").packages["node_modules/ws"].version')
npm install --prefix "$lab/ws" --ignore-scripts --no-audit --no-fund "ws@$ws_version"
NODE_PATH="$lab/ws/node_modules" PIMATE_DEMO_SCREENS=list start_fixture 1
start_fixture 2
result="$build/Tests-$(date +%s).xcresult"
TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 "${xcb[@]}" -only-testing:PiMateTests -resultBundlePath "$result" test-without-building
[[ ! -e $link-1 && ! -e $link-2 ]]
# One fresh link for the failure/retry and edit journey; do not reuse the screenshot fixture.
PIMATE_DEMO_REJECT_FIRST_TREE=1 start_fixture "$((${#links[@]} + 1))"
xcrun simctl pbcopy "$id" <"$file"; rm -f "$file"
TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 "${xcb[@]}" -only-testing:PiMateUITests/ProbeTests/testSheetOperations test-without-building
# Exercise the two first-pairing routes separately; each uses a fresh link and test-store namespace.
agent_start=$SECONDS
PIMATE_DEMO_CHAT_FAILURE=1 start_fixture "$((${#links[@]} + 1))"
xcrun simctl pbcopy "$id" <"$file"; rm -f "$file"
TEST_RUNNER_PIMATE_CHAT_FIXTURE=1 TEST_RUNNER_PIMATE_DEMO_CHAT_FAILURE=1 "${xcb[@]}" -only-testing:PiMateUITests/AgentReviewJourney/testChatPages test-without-building
printf 'AgentReviewJourney (#52; fixture + UI): %d s\n' "$((SECONDS-agent_start))" | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
NODE_PATH="$lab/ws/node_modules" PIMATE_DEMO_SCREENS=list PIMATE_DEMO_GITHUB_TABLES=1 PIMATE_DEMO_DRAWER_BOT=1 start_fixture "$((${#links[@]} + 1))"
xcrun simctl pbcopy "$id" <"$file"; rm -f "$file"
machine_result="$build/Machine-$(date +%s).xcresult"
TEST_RUNNER_PIMATE_SCREEN_FIXTURE=list TEST_RUNNER_PIMATE_DEMO_GITHUB_TABLES=1 TEST_RUNNER_PIMATE_DEMO_DRAWER_BOT=1 "${xcb[@]}" -only-testing:PiMateUITests/ProbeTests/testMachineScreens -resultBundlePath "$machine_result" test-without-building
xcrun xcresulttool export attachments --path "$machine_result" --output-path "$build/attachments/machine"
failed=0
for variant in light dark large; do
  mode=light; size=large
  [[ $variant != dark ]] || mode=dark
  [[ $variant != large ]] || size=extra-extra-extra-large
  xcrun simctl ui "$id" appearance "$mode"
  xcrun simctl ui "$id" content_size "$size"
  start_fixture "$((${#links[@]} + 1))"
  xcrun simctl pbcopy "$id" <"$file"; rm -f "$file"
  result="$build/$variant-$(date +%s).xcresult"
  journey() {
    TEST_RUNNER_PIMATE_REQUIRE_DEMO=1 TEST_RUNNER_PIMATE_VARIANT="$variant" "${xcb[@]}" -only-testing:PiMateUITests/ProbeTests/testJourney -resultBundlePath "$result" test-without-building
  }
  # Retry only XCTest's simulator launch failure, before the journey has consumed its link.
  if journey 2>&1 | tee "$lab/journey.log"; then ok=1
  elif grep -Eq 'testJourney] : Failed to get launch progress.*reason: Busy .*is installing or uninstalling' "$lab/journey.log"; then
    result="${result%.xcresult}-retry.xcresult"
    if journey; then ok=1; else ok=0; fi
  else ok=0; fi
  if (( ok )); then
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
