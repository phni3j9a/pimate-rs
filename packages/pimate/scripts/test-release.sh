#!/usr/bin/env bash
# Exercise the same pre-publish guard with a local history, no tags or publishing.
set -euo pipefail
check=$(realpath "$(dirname "$0")/check-release.sh")
lab=$(mktemp -d); trap 'rm -rf "$lab"' EXIT
cd "$lab"; git init -q
mkdir -p packages/pimate package src
printf '[package]\nname="pimate-host"\nversion="1.0.0"\nedition="2024"\n' > Cargo.toml
touch src/lib.rs; cargo generate-lockfile
printf '{"name":"@phni3j9a/pimate","version":"1.0.0"}' > packages/pimate/package.json
cp packages/pimate/package.json package/; tar czf pack.tgz package/package.json
git add Cargo.* src packages; git -c user.name=test -c user.email=test@example.invalid commit -qm approved
export GITHUB_REF_NAME=host-v1.0.0 GITHUB_SHA=$(git rev-parse HEAD)
git update-ref refs/remotes/origin/main HEAD; git fetch -q --no-tags . HEAD
bash "$check" pack.tgz
reject() { if bash "$check" pack.tgz; then echo 'release guard accepted invalid input' >&2; exit 1; fi; }
GITHUB_REF_NAME=host-v2.0.0 reject
sed -i 's/1.0.0/2.0.0/' packages/pimate/package.json; reject; git restore packages
sed -i 's/1.0.0/2.0.0/' Cargo.toml; cargo generate-lockfile; reject; git restore Cargo.toml Cargo.lock
git -c user.name=test -c user.email=test@example.invalid commit --allow-empty -qm unmerged
export GITHUB_SHA=$(git rev-parse HEAD); git fetch -q --no-tags . HEAD
reject
