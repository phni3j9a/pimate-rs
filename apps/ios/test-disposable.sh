#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
id=$(xcrun simctl create PiMate-Test com.apple.CoreSimulator.SimDeviceType.iPhone-15)
trap 'xcrun simctl delete "$id"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
./test.sh "$id"
