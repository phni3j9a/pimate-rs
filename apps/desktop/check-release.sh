#!/usr/bin/env bash
set -euo pipefail
tag=${1:?expected desktop-vX.Y.Z}
[[ $tag =~ ^desktop-v[0-9]+\.[0-9]+\.[0-9]+$ ]]
git fetch --no-tags origin +refs/heads/main:refs/remotes/origin/main "+refs/tags/$tag:refs/verification/desktop-tag"
sha=$(git rev-parse 'refs/verification/desktop-tag^{commit}')
git merge-base --is-ancestor "$sha" refs/remotes/origin/main
version=$(git show "$sha:apps/desktop/Cargo.toml" | python3 -c 'import sys,tomllib; print(tomllib.load(sys.stdin.buffer)["package"]["version"])')
test "$tag" = "desktop-v$version"
printf '%s\n' "$sha"
