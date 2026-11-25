#!/usr/bin/env bash
# Exercise the main-side guard against a disposable local origin (no network).
set -euo pipefail
check=$(realpath "$(dirname "$0")/../check-release.sh")
lab=$(mktemp -d); trap 'rm -rf "$lab"' EXIT
if bash "$(dirname "$check")/tests/linux-session.sh" package "$lab/out"; then
  echo 'package smoke accepted a missing executable' >&2; exit 1
fi
export GITHUB_OUTPUT="$lab/signing"
export P12='' P12_PASSWORD='' ASC_KEY='' ASC_KEY_ID='' ASC_ISSUER=''
bash "$(dirname "$check")/notarize.sh"
grep -Fx 'signing=ad-hoc・未公証' "$GITHUB_OUTPUT"
if ASC_KEY_ID=incomplete bash "$(dirname "$check")/notarize.sh"; then
  echo 'partial Apple credentials accepted' >&2; exit 1
fi
cd "$lab"; git init -qb main; git remote add origin .
git config user.name test; git config user.email test@example.invalid
mkdir -p apps/desktop
printf '[package]\nversion="0.2.0"\n' > apps/desktop/Cargo.toml
git add .; git commit -qm approved; expected=$(git rev-parse HEAD)
tag=desktop-v0.2.0; git update-ref "refs/tags/$tag" HEAD; git commit --allow-empty -qm newer
test "$(bash "$check" "$tag")" = "$expected"
reject() { if bash "$check" "$1"; then echo "incorrectly accepted $1" >&2; exit 1; fi; }
reject main; git update-ref refs/tags/desktop-v2.0.0 HEAD; reject desktop-v2.0.0
git checkout -qb unmerged; git commit --allow-empty -qm unmerged
git update-ref "refs/tags/$tag" HEAD; reject "$tag"
