#!/usr/bin/env bash
# A disposable origin with local refs only: no pushes, releases or credentials.
set -euo pipefail
check=$(realpath "$(dirname "$0")/check-release.sh")
lab=$(mktemp -d); trap 'rm -rf "$lab"' EXIT
cd "$lab"; git init -qb main; git remote add origin .
git config user.name test; git config user.email test@example.invalid
mkdir -p packages/pimate package crates/pimate-host
printf '[package]\nname="pimate-host"\nversion="1.0.0"\n' > crates/pimate-host/Cargo.toml
printf '{"name":"@phni3j9a/pimate","version":"1.0.0"}' > packages/pimate/package.json
cp packages/pimate/package.json package/; tar czf pack.tgz package/package.json
git add crates packages; git commit -qm approved
pinned=$(git rev-parse HEAD); tag=host-v1.0.0
point_tag() { git update-ref "refs/tags/$tag" HEAD; }
point_tag; git commit --allow-empty -qm newer-main
test "$(bash "$check" "$tag")" = "$pinned"
bash "$check" "$tag" "$pinned" pack.tgz
reject() { if bash "$check" "$@"; then echo 'release guard accepted invalid input' >&2; exit 1; fi; }
reject refs/heads/main; reject host-v2.0.0
sed -i 's/1.0.0/2.0.0/' package/package.json; tar czf pack.tgz package/package.json
reject "$tag" "$pinned" pack.tgz
cp packages/pimate/package.json package/; tar czf pack.tgz package/package.json
point_tag; reject "$tag" "$pinned" pack.tgz  # tag moved after build
sed -i 's/1.0.0/2.0.0/' packages/pimate/package.json; git commit -qam bad-npm; point_tag; reject "$tag"
git restore --source="$pinned" packages; sed -i 's/1.0.0/2.0.0/' crates/pimate-host/Cargo.toml
git commit -qam bad-cargo; point_tag; reject "$tag"
git checkout -qb unmerged "$pinned"; git commit --allow-empty -qm unmerged; point_tag; reject "$tag"
