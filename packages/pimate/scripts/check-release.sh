#!/usr/bin/env bash
# Read manifests as data; no release-tag code runs in the privileged job.
set -euo pipefail
tag=${1:?expected host-vX.Y.Z}
[[ "$tag" =~ ^host-v[0-9]+\.[0-9]+\.[0-9]+$ ]]
git fetch --no-tags origin +refs/heads/main:refs/remotes/origin/main "+refs/tags/$tag:refs/verification/release-tag"
commit=$(git rev-parse 'refs/verification/release-tag^{commit}')
git merge-base --is-ancestor "$commit" refs/remotes/origin/main
version=${tag#host-v}
npm_version() { jq -er 'select(.name == "@phni3j9a/pimate").version'; }
test "$(git show "$commit:packages/pimate/package.json" | npm_version)" = "$version"
test "$(git show "$commit:crates/pimate-host/Cargo.toml" | python3 -c 'import sys,tomllib; print(tomllib.load(sys.stdin.buffer)["package"]["version"])')" = "$version"
if (( $# != 1 )); then
  test "$#" = 3
  test "$commit" = "$2"
  test "$(tar -xOf "$3" package/package.json | npm_version)" = "$version"
fi
printf '%s\n' "$commit"
