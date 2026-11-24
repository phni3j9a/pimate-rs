#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
export CARGO_TARGET_DIR="$PWD/target"
scripts/build-ios.sh
cd apps/ios
xcodegen generate
