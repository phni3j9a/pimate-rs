#!/usr/bin/env bash
# Local compatibility check against a read-only upstream checkout with TypeScript installed.
set -euo pipefail
cd "$(dirname "$0")/.."
upstream=$(realpath "${1:?usage: check-upstream.sh /path/to/PiMate [--write-fixtures]}")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
ln -s "$upstream/packages/protocol/src" "$tmp/upstream"
ln -s "$PWD/bindings/ts/index.ts" "$tmp/generated.ts"
cp scripts/{fixtures,compatibility}.ts "$tmp/"
"$upstream/node_modules/.bin/tsc" --noEmit --strict --exactOptionalPropertyTypes --target es2022 \
  --module esnext --moduleResolution bundler --allowImportingTsExtensions --skipLibCheck "$tmp/"{fixtures,compatibility}.ts \
  "$upstream/packages/protocol/src/index.ts" --preserveSymlinks false
node "$tmp/fixtures.ts" > "$tmp/wire.json"
fixture=crates/pimate-protocol/tests/fixtures/wire.json
if [[ ${2:-} == --write-fixtures ]]; then cp "$tmp/wire.json" "$fixture"; else diff -u "$fixture" "$tmp/wire.json"; fi
